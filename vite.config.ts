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
	server: {
		port: 5173,
		strictPort: true,
		watch: {
			// src-tauri（Rust のソース・target・resources）は Vite の監視から外す。
			// target は巨大になりうるため、監視が起動時間を支配する（docs/design.md 11 章「開発時の起動と再ビルド」）。
			// Rust 側の変更は Tauri CLI が監視して再ビルドする
			ignored: ['**/src-tauri/**']
		}
	}
});
