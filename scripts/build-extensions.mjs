// 組み込みの拡張（Office 抽出器の WASM）をビルドして、拡張フォルダに配置する（設計書 11.1）。
//
// - `cargo build --release --target wasm32-wasip1 -p office-extractor` を実行し、3 つの .wasm を
//   src-tauri/app/resources/extensions/hikae.builtin.office/ に置く
// - .wasm はリポジトリに入れない（.gitignore）。extension.json は手で書いてリポジトリに入れる
// - wasm32-wasip1 の標準ライブラリは src-tauri/rust-toolchain.toml の targets により rustup が取得する
// - CARGO_TARGET_DIR が設定されていれば、その下の成果物を使う
// - 表示拡張（hikae.builtin.preview）の UI を vite.ext.config.ts でビルドし、ui/ に置く（wasm の後）
//
// 使い方: node scripts/build-extensions.mjs

import { spawnSync } from 'node:child_process';
import { copyFile, readFile, stat } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const TARGET = 'wasm32-wasip1';
// manifest（extension.json）の extractors[].entry と同じ名前にする
const BINS = ['extract-docx', 'extract-xlsx', 'extract-pptx'];

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const tauriDir = join(root, 'src-tauri');
const extDir = join(tauriDir, 'app', 'resources', 'extensions', 'hikae.builtin.office');
// 表示拡張（hikae.builtin.preview）の UI の出力先（vite.ext.config.ts の outDir と同じ）
const uiDir = join(tauriDir, 'app', 'resources', 'extensions', 'hikae.builtin.preview', 'ui');

function log(message) {
	console.log(`[build-extensions] ${message}`);
}

function fail(message) {
	console.error(`[build-extensions] ${message}`);
	process.exit(1);
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
		const head = (await readFile(src)).subarray(0, 4);
		if (head.toString('latin1') !== '\0asm') {
			fail(`${bin}.wasm の形式が不正です`);
		}
		await copyFile(src, dest);
	} catch (err) {
		if (err?.code === 'ENOENT') {
			fail(`${bin}.wasm が生成されていません`);
		}
		throw err;
	}
	const size = (await stat(dest)).size;
	log(`${bin}.wasm を配置しました（${(size / 1024).toFixed(0)} KiB）`);
}

// 表示拡張（iframe の UI）。vite.ext.config.ts で extensions-ui/hikae.builtin.preview をビルドし、
// 拡張フォルダの ui/ に置く（設計書 11 章）。manifest の renderers の入口（ui/index.html）が無いと登録されない
log('表示拡張の UI をビルドします');
try {
	const { build } = await import('vite');
	await build({ configFile: join(root, 'vite.ext.config.ts'), logLevel: 'warn' });
} catch {
	fail('表示拡張の UI のビルドに失敗しました（pnpm install の後に実行してください）');
}
try {
	await stat(join(uiDir, 'index.html'));
} catch {
	fail('表示拡張の UI（ui/index.html）が生成されていません');
}
log('表示拡張の UI を配置しました');
