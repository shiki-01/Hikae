import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import masterCSS from '@master/css.vite';
import { defineConfig } from 'vite';

// Tauri の dev 実行時に固定ポートを使う
export default defineConfig({
	plugins: [
		masterCSS({ mode: 'static' }),
		// Tauri から読み込む SPA 構成（SSR なし）
		sveltekit({ adapter: adapter({ fallback: 'index.html' }) })
	],
	clearScreen: false,
	server: { port: 5173, strictPort: true }
});
