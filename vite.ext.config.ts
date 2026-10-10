// 組み込みの表示拡張（iframe の UI）のビルド設定（設計書 11 章）。`scripts/build-extensions.mjs` から呼ぶ。
// アプリ本体の SvelteKit は使わず、Svelte と Master CSS だけで、拡張フォルダの ui/ に静的ファイルを出す。
// - base は相対パス（`ext://localhost/<拡張 ID>/ui/` の下で読めるように）
// - module preload の polyfill は入れない（インラインのスクリプトを作らない。CSP の script-src に合わせる）
// - 画像・フォントの data: 化は行わない（font-src 'self' に合わせる）
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import { masterCssPlugin } from './vite.shared.ts';

const repo = fileURLToPath(new URL('.', import.meta.url));
const extensionRoot = resolve(repo, 'extensions-ui/hikae.builtin.preview');
const outDir = resolve(repo, 'src-tauri/app/resources/extensions/hikae.builtin.preview/ui');

export default defineConfig({
	root: extensionRoot,
	base: './',
	plugins: [masterCssPlugin(), svelte()],
	resolve: {
		// アプリ本体と同じ `#lib`（package.json の imports と同じ場所）
		alias: [
			{ find: /^#lib$/, replacement: resolve(repo, 'src/lib/index.ts') },
			{ find: /^#lib\/(.*)$/, replacement: resolve(repo, 'src/lib') + '/$1' }
		]
	},
	build: {
		outDir,
		emptyOutDir: true,
		assetsInlineLimit: 0,
		modulePreload: { polyfill: false },
		sourcemap: false
	}
});
