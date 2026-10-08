// Windows 用の同梱 git（MinGit）を取得して src-tauri/app/resources/git/ に展開する。
//
// - 取得元は git-for-windows/git の公式リリースのみ。バージョンと SHA-256 をこのファイルに固定する
// - ダウンロード後に必ず SHA-256 を検証し、不一致なら破棄して失敗する（検証前は展開も実行もしない）
// - 展開後に不要なファイル（GUI の credential manager など）を削り、同梱 git で最小の流れを実行して確認する
// - 展開済みで版が一致していれば何もしない（再取得しない）
// - Windows 以外では何もしない（macOS は scripts/build-git-macos.sh）
//
// MinGit を更新するときは、TAG / ASSET / SHA256 を 3 つ同時に書き換える。
// SHA256 は `gh api repos/git-for-windows/git/releases/tags/<TAG> --jq '.assets[] | [.name,.digest] | @tsv'` で確認する。
//
// 使い方: node scripts/fetch-git-windows.mjs [--force]

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, rename, rm, writeFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

const TAG = 'v2.56.0.windows.2';
const ASSET = 'MinGit-2.56.0.2-64-bit.zip';
const SHA256 = 'da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be';
const GIT_VERSION_OUTPUT = 'git version 2.56.0.windows.2';
const URL = `https://github.com/git-for-windows/git/releases/download/${TAG}/${ASSET}`;

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const resourcesDir = join(root, 'src-tauri', 'app', 'resources');
const destDir = join(resourcesDir, 'git');
const stagingDir = join(resourcesDir, '.git-staging');
const markerPath = join(destDir, '.hikae-git-version');
const marker = `${TAG} ${ASSET} ${SHA256}\n`;

// 削るファイル（Hikae は GCM・GUI・Scalar・docx 変換を使わない。credential helper は自前のものを渡す）。
// ucrt64/bin 直下の名前（完全一致）と、名前の接頭辞
const REMOVE_BIN_EXACT = new Set([
	'Atlassian.Bitbucket.dll',
	'GitHub.dll',
	'GitLab.dll',
	'HarfBuzzSharp.dll',
	'MicroCom.Runtime.dll',
	'SkiaSharp.dll',
	'libSkiaSharp.dll',
	'libHarfBuzzSharp.dll',
	'av_libglesv2.dll',
	'msalruntime.dll',
	'gcmcore.dll',
	'git-credential-manager.exe',
	'git-credential-manager.exe.config',
	'git-credential-helper-selector.exe',
	'git-askpass.exe',
	'git-askyesno.exe',
	'blocked-file-util.exe',
	'docx-strip-pii.exe',
	'docx2txt.exe',
	'scalar.exe',
	'git-update-git-for-windows'
]);
const REMOVE_BIN_PREFIX = ['Avalonia.', 'System.', 'Microsoft.'];
// 展開先からの相対パスで削るもの
const REMOVE_PATHS = [
	'ucrt64/libexec/git-core/git-credential-wincred.exe',
	'ucrt64/share/doc',
	'ucrt64/share/bash-completion'
];

function log(message) {
	console.log(`[fetch-git-windows] ${message}`);
}

async function sha256Of(path) {
	const hash = createHash('sha256');
	await pipeline(createReadStream(path), hash);
	return hash.digest('hex');
}

async function download(url, path) {
	const res = await fetch(url, { redirect: 'follow' });
	if (!res.ok || !res.body) {
		throw new Error(`ダウンロードに失敗しました: HTTP ${res.status}`);
	}
	await pipeline(Readable.fromWeb(res.body), createWriteStream(path));
}

function expandZip(zipPath, outDir) {
	// 標準の PowerShell（Expand-Archive）で展開する。パスはシングルクォートで囲む
	const quote = (p) => `'${p.replaceAll("'", "''")}'`;
	const r = spawnSync(
		'powershell.exe',
		[
			'-NoProfile',
			'-NonInteractive',
			'-Command',
			`Expand-Archive -LiteralPath ${quote(zipPath)} -DestinationPath ${quote(outDir)} -Force`
		],
		{ stdio: 'inherit' }
	);
	if (r.status !== 0) {
		throw new Error('zip の展開に失敗しました');
	}
}

async function trim(dir) {
	const binDir = join(dir, 'ucrt64', 'bin');
	for (const name of await readdir(binDir)) {
		if (REMOVE_BIN_EXACT.has(name) || REMOVE_BIN_PREFIX.some((p) => name.startsWith(p))) {
			await rm(join(binDir, name), { force: true });
		}
	}
	for (const rel of REMOVE_PATHS) {
		await rm(join(dir, rel), { recursive: true, force: true });
	}
}

/** 展開した git で、init → 設定 → add → commit → log の最小の流れが通ることを確認する */
async function smokeTest(dir) {
	const git = join(dir, 'cmd', 'git.exe');
	const exec = join(dir, 'ucrt64', 'libexec', 'git-core');
	for (const needed of [
		git,
		join(dir, 'ucrt64', 'bin', 'git-remote-https.exe'),
		join(dir, 'usr', 'bin', 'sh.exe'),
		join(dir, 'LICENSE.txt')
	]) {
		if (!existsSync(needed)) throw new Error(`同梱 git に必要なファイルがありません: ${needed}`);
	}
	const work = await mkdtemp(join(tmpdir(), 'hikae-git-smoke-'));
	// 親の環境を引き継がない（PATH も渡さない）。同梱 git だけで動くことを確かめる
	const env = {
		SystemRoot: process.env.SystemRoot ?? 'C:\\Windows',
		TEMP: tmpdir(),
		TMP: tmpdir(),
		GIT_CONFIG_GLOBAL: 'NUL',
		GIT_CONFIG_NOSYSTEM: '1',
		GIT_TERMINAL_PROMPT: '0',
		GIT_EXEC_PATH: exec,
		LC_ALL: 'C'
	};
	const run = (args, extra = {}) => {
		const r = spawnSync(git, args, { cwd: work, env: { ...env, ...extra }, encoding: 'utf8' });
		if (r.status !== 0) {
			throw new Error(`git ${args.join(' ')} が失敗しました (${r.status}): ${r.stderr}`);
		}
		return r.stdout.trim();
	};
	try {
		const version = run(['--version']);
		if (version !== GIT_VERSION_OUTPUT) {
			throw new Error(`想定外のバージョンです: ${version}`);
		}
		run(['init', '-q', '.']);
		run(['config', '--local', 'user.name', 'Hikae Smoke']);
		run(['config', '--local', 'user.email', 'smoke@example.invalid']);
		await writeFile(join(work, 'a.txt'), 'hello\n');
		run(['add', 'a.txt']);
		run(['-c', 'core.hooksPath=', 'commit', '-q', '-m', 'smoke']);
		const logged = run(['log', '--format=%s']);
		if (logged !== 'smoke') throw new Error(`log の結果が想定と違います: ${logged}`);
		// sh が同梱の usr/bin から見つかること（`!` 形式の credential helper に必要）
		const sh = run(['-c', 'alias.hikae-smoke=!echo shell-ok', 'hikae-smoke']);
		if (sh !== 'shell-ok') throw new Error(`sh の実行に失敗しました: ${sh}`);
	} finally {
		await rm(work, { recursive: true, force: true });
	}
}

async function main() {
	if (process.platform !== 'win32') {
		log('Windows 以外のためスキップします');
		return;
	}
	const force = process.argv.includes('--force');

	if (!force && existsSync(markerPath) && existsSync(join(destDir, 'cmd', 'git.exe'))) {
		if ((await readFile(markerPath, 'utf8')) === marker) {
			log(`取得済みです (${TAG})`);
			return;
		}
	}

	await mkdir(resourcesDir, { recursive: true });
	const work = await mkdtemp(join(tmpdir(), 'hikae-mingit-'));
	try {
		const zipPath = join(work, ASSET);
		log(`ダウンロード: ${URL}`);
		await download(URL, zipPath);

		const actual = await sha256Of(zipPath);
		if (actual !== SHA256) {
			await rm(zipPath, { force: true });
			throw new Error(`SHA-256 が一致しません。破棄しました\n  期待: ${SHA256}\n  実際: ${actual}`);
		}
		log('SHA-256 の検証に成功しました');

		await rm(stagingDir, { recursive: true, force: true });
		await mkdir(stagingDir, { recursive: true });
		expandZip(zipPath, stagingDir);
		await trim(stagingDir);
		await smokeTest(stagingDir);

		// 検証済みのものだけを本番の場所へ移す（.gitkeep は残す）
		await rm(destDir, { recursive: true, force: true });
		await rename(stagingDir, destDir);
		await writeFile(join(destDir, '.gitkeep'), '');
		await writeFile(markerPath, marker);
		log(`展開しました: ${destDir}`);
	} catch (e) {
		await rm(stagingDir, { recursive: true, force: true });
		throw e;
	} finally {
		await rm(work, { recursive: true, force: true });
	}
}

main().catch((e) => {
	console.error(`[fetch-git-windows] 失敗: ${e.message}`);
	process.exit(1);
});
