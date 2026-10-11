// 組み込みの拡張（Office 抽出器の WASM）をビルドして、拡張フォルダに配置する（設計書 11.1）。
//
// - `cargo build --release --target wasm32-wasip1 -p office-extractor` を実行し、3 つの .wasm を
//   src-tauri/app/resources/extensions/hikae.builtin.office/ に置く
// - .wasm はリポジトリに入れない（.gitignore）。extension.json は手で書いてリポジトリに入れる
// - wasm32-wasip1 の標準ライブラリは src-tauri/rust-toolchain.toml の targets により rustup が取得する
// - CARGO_TARGET_DIR が設定されていれば、その下の成果物を使う
// - 表示拡張（hikae.builtin.preview）の UI を vite.ext.config.ts でビルドし、ui/ に置く（wasm の後）
// - 出力は内容が違うファイルだけ書き換える（同じ内容のファイルの更新時刻は変えない）。
//   更新時刻が変わると tauri-build が再実行されて、app の再コンパイルが起きるため（docs/design.md 11 章「開発時の起動と再ビルド」）
// - 入力（拡張の UI と wasm のソース）の内容が前回と同じで、出力が揃っていればビルドを飛ばす。
//   キャッシュは node_modules/.cache/hikae-build-extensions.json。--force で必ず全部ビルドする
//
// 使い方: node scripts/build-extensions.mjs [--force]

import { spawnSync } from 'node:child_process';
import { readFile, rm, stat } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
	hashInputs,
	readCache,
	shouldSkip,
	syncFile,
	syncTree,
	writeCache
} from './build-extensions-cache.mjs';

const TARGET = 'wasm32-wasip1';
// manifest（extension.json）の extractors[].entry と同じ名前にする
const BINS = ['extract-docx', 'extract-xlsx', 'extract-pptx'];

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const tauriDir = join(root, 'src-tauri');
const extDir = join(tauriDir, 'app', 'resources', 'extensions', 'hikae.builtin.office');
// 表示拡張（hikae.builtin.preview）の UI の出力先（vite.ext.config.ts の outDir と同じ）
const uiDir = join(tauriDir, 'app', 'resources', 'extensions', 'hikae.builtin.preview', 'ui');
// ビルドの中間出力（Vite の出力先を差し替える）と、入力ハッシュのキャッシュ
const cacheDir = join(root, 'node_modules', '.cache');
const scratchUiDir = join(cacheDir, 'hikae-build-extensions', 'ui');
const cachePath = join(cacheDir, 'hikae-build-extensions.json');

// ビルドの出力に効く入力。迷ったら含める（含め漏れは、変更してもビルドし直さない原因になる）
const INPUTS = [
	'src/lib',
	'extensions-ui',
	'vite.ext.config.ts',
	'vite.shared.ts',
	'tsconfig.json',
	'package.json',
	'pnpm-lock.yaml',
	'scripts/build-extensions.mjs',
	'scripts/build-extensions-cache.mjs',
	'src-tauri/Cargo.toml',
	'src-tauri/Cargo.lock',
	'src-tauri/rust-toolchain.toml',
	'src-tauri/.cargo',
	'src-tauri/extensions',
	'src-tauri/crates/core-preview/Cargo.toml',
	'src-tauri/crates/core-preview/src',
	'src-tauri/app/resources/extensions/hikae.builtin.office/extension.json',
	'src-tauri/app/resources/extensions/hikae.builtin.preview/extension.json'
];

function log(message) {
	console.log(`[build-extensions] ${message}`);
}

function fail(message) {
	console.error(`[build-extensions] ${message}`);
	process.exit(1);
}

/** 出力（wasm・UI の入口・extension.json）がすべて揃っているか */
async function outputsPresent() {
	const files = [
		...BINS.map((bin) => join(extDir, `${bin}.wasm`)),
		join(uiDir, 'index.html'),
		join(extDir, 'extension.json')
	];
	for (const file of files) {
		try {
			await stat(file);
		} catch {
			return false;
		}
	}
	return true;
}

const force = process.argv.includes('--force');

// 入力のハッシュは、ビルドの前に取る（ビルド中に変わったファイルは、次回のビルドで拾われる）
const inputHash = await hashInputs(root, INPUTS);
const cache = await readCache(cachePath);
if (shouldSkip({ force, cache, inputHash, outputsPresent: await outputsPresent() })) {
	log('入力に変更がないため、ビルドを飛ばします（変更なし。スキップ。--force で強制）');
	process.exit(0);
}

const build = spawnSync(
	'cargo',
	['build', '--release', '--target', TARGET, '-p', 'office-extractor'],
	{ cwd: tauriDir, stdio: 'inherit' }
);
if (build.status !== 0) {
	fail(
		'cargo build に失敗しました（wasm32-wasip1 の target が無い場合は rustup target add を確認してください）'
	);
}

const targetDir = process.env.CARGO_TARGET_DIR
	? resolve(tauriDir, process.env.CARGO_TARGET_DIR)
	: join(tauriDir, 'target');
const outDir = join(targetDir, TARGET, 'release');

// マニフェストは手で書いてリポジトリに入れるため、無ければ配置先を作らず止める
try {
	await stat(join(extDir, 'extension.json'));
} catch {
	fail('extension.json が見つかりません');
}

for (const bin of BINS) {
	const src = join(outDir, `${bin}.wasm`);
	const dest = join(extDir, `${bin}.wasm`);
	try {
		// WebAssembly のマジックナンバー（\0asm）を確かめてから配置する
		const bytes = await readFile(src);
		if (bytes.subarray(0, 4).toString('latin1') !== '\0asm') {
			fail(`${bin}.wasm の形式が不正です`);
		}
		const changed = await syncFile(src, dest);
		const size = (await stat(dest)).size;
		log(
			`${bin}.wasm を${changed ? '配置しました' : 'そのまま（変更なし）'}（${(size / 1024).toFixed(0)} KiB）`
		);
	} catch (err) {
		if (err?.code === 'ENOENT') {
			fail(`${bin}.wasm が生成されていません`);
		}
		throw err;
	}
}

// 表示拡張（iframe の UI）。vite.ext.config.ts で extensions-ui/hikae.builtin.preview をビルドし、
// 中間フォルダに出してから、差分だけを拡張フォルダの ui/ に置く（設計書 11 章）。
// manifest の renderers の入口（ui/index.html）が無いと登録されない
log('表示拡張の UI をビルドします');
await rm(scratchUiDir, { recursive: true, force: true });

// tsconfig の継承元（`$app/tsconfig`）は `.svelte-kit/tsconfig.json`（`svelte-kit sync` の生成物）にある。
// 新しい clone や CI では無く、無いと UI のビルドが「Tsconfig not found」で失敗するため、先に生成する
try {
	await stat(join(root, '.svelte-kit', 'tsconfig.json'));
} catch {
	log('.svelte-kit が無いため、svelte-kit sync を実行します');
	const sync = spawnSync(
		process.execPath,
		[join(root, 'node_modules', '@sveltejs', 'kit', 'svelte-kit.js'), 'sync'],
		{ cwd: root, stdio: 'inherit' }
	);
	if (sync.status !== 0) {
		fail('svelte-kit sync に失敗しました（pnpm install の後に実行してください）');
	}
}

try {
	const { build: viteBuild } = await import('vite');
	await viteBuild({
		configFile: join(root, 'vite.ext.config.ts'),
		logLevel: 'warn',
		build: { outDir: scratchUiDir }
	});
} catch (err) {
	// 原因が分からないと直せないため、エラーの本文（先頭の数行）を出す
	const detail = String(err?.message ?? err).split('\n').slice(0, 6).join('\n');
	console.error(detail);
	fail('表示拡張の UI のビルドに失敗しました（pnpm install の後に実行してください）');
}
try {
	await stat(join(scratchUiDir, 'index.html'));
} catch {
	fail('表示拡張の UI（ui/index.html）が生成されていません');
}
const ui = await syncTree(scratchUiDir, uiDir);
log(
	`表示拡張の UI を配置しました（書き換え ${ui.written.length} 件、削除 ${ui.removed.length} 件、変更なし ${ui.unchanged} 件）`
);

await writeCache(cachePath, inputHash);
