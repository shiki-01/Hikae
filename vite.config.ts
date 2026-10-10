import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import { defineConfig } from 'vite';
import { masterCssPlugin } from './vite.shared.ts';

// Tauri の dev 実行時に固定ポートを使う
export default defineConfig({
	plugins: [
		masterCssPlugin(),
		// Tauri から読み込む SPA 構成（SSR なし）
		sveltekit({ adapter: adapter({ fallback: 'index.html' }) })
	],
	clearScreen: false,
	server: { port: 5173, strictPort: true }
});
