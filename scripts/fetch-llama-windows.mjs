// Windows 用の内蔵 llama-server（llama.cpp の公式リリース、CPU 版）を取得して src-tauri/app/resources/llama/ に展開する。
//
// - 取得元は ggml-org/llama.cpp の公式リリースのみ。バージョンと SHA-256 をこのファイルに固定する
// - ダウンロード後に必ず SHA-256 を検証し、不一致なら破棄して失敗する（検証前は展開も実行もしない）
// - 残すのは llama-server.exe と、同じフォルダの DLL（ggml・llama などの実行に必要なもの）と LICENSE だけ。
//   それ以外の実行ファイル（llama-cli など）は含めない
// - 展開後に `llama-server.exe --version` が通ることを確認してから、本番の場所へ移す
// - 展開済みで版が一致していれば何もしない（再取得しない）
// - Windows 以外では何もしない（macOS は HIKAE_LLAMA_SERVER_PATH と PATH の探索に任せる。設計書 10.5）
//
// llama.cpp を更新するときは、TAG / ASSET / SHA256 を同時に書き換える。
// SHA256 は `gh api repos/ggml-org/llama.cpp/releases/tags/<TAG> --jq '.assets[] | [.name,.digest] | @tsv'` で確認する。
//
// 使い方: node scripts/fetch-llama-windows.mjs [--force]

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, existsSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

const TAG = 'b11540';
const ASSET = 'llama-b11540-bin-win-cpu-x64.zip';
const SHA256 = '85cae1b982145d1b315e56e7343524b301a11f0fdf2c7c9dd973c26daba4b3b5';
const URL = `https://github.com/ggml-org/llama.cpp/releases/download/${TAG}/${ASSET}`;
const SERVER_EXE = 'llama-server.exe';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const resourcesDir = join(root, 'src-tauri', 'app', 'resources');
const destDir = join(resourcesDir, 'llama');
const stagingDir = join(resourcesDir, '.llama-staging');
const markerPath = join(destDir, '.hikae-llama-version');
const marker = `${TAG} ${ASSET} ${SHA256}\n`;

function log(message) {
	console.log(`[fetch-llama-windows] ${message}`);
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

/** 展開先の直下だけを見て、必要なものだけを残す。zip の中のフォルダ構成は平らに扱う */
async function trim(dir) {
	const keep = (name) => {
		const lower = name.toLowerCase();
		if (lower === SERVER_EXE) return true;
		if (lower.endsWith('.dll')) return true;
		if (lower === 'license' || lower === 'license.txt') return true;
		return false;
	};
	for (const name of await readdir(dir, { withFileTypes: true })) {
		if (name.isDirectory()) {
			// サブフォルダは実行に使わないため削る
			await rm(join(dir, name.name), { recursive: true, force: true });
		} else if (!keep(name.name)) {
			await rm(join(dir, name.name), { force: true });
		}
	}
	if (!existsSync(join(dir, SERVER_EXE))) {
		throw new Error(`zip に ${SERVER_EXE} がありません`);
	}
}

/** 展開した llama-server が起動でき、想定の版を名乗ることを確かめる */
function smokeTest(dir) {
	const exe = join(dir, SERVER_EXE);
	const r = spawnSync(exe, ['--version'], { cwd: dir, encoding: 'utf8', timeout: 60000 });
	const text = `${r.stdout ?? ''}\n${r.stderr ?? ''}`;
	if (r.status !== 0) {
		throw new Error(`${SERVER_EXE} --version が失敗しました (${r.status})`);
	}
	if (!text.includes(TAG.replace(/^b/, ''))) {
		throw new Error(`想定外の版です（期待: ${TAG}）`);
	}
}

async function main() {
	if (process.platform !== 'win32') {
		log('Windows 以外のためスキップします');
		return;
	}
	const force = process.argv.includes('--force');

	if (!force && existsSync(markerPath) && existsSync(join(destDir, SERVER_EXE))) {
		if ((await readFile(markerPath, 'utf8')) === marker) {
			log(`取得済みです (${TAG})`);
			return;
		}
	}

	await mkdir(resourcesDir, { recursive: true });
	const work = await mkdtemp(join(tmpdir(), 'hikae-llama-'));
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
		smokeTest(stagingDir);

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
	console.error(`[fetch-llama-windows] 失敗: ${e.message}`);
	process.exit(1);
});
