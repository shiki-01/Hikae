import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';
import masterCSS from '@master/css.vite';
import { defineConfig } from 'vite';

// Master CSS rc.88 の Svelte 用抽出は、{:else} 側の分岐に書いたクラスを拾えない。
// .svelte はファイル全体を単語に分け、候補として渡す（無効な単語は Master CSS 側の検証で捨てられる）
const svelteFullTextAdapter = {
	name: 'svelte-full-text',
	test: (source: string) =>
		/\.svelte(?:\?|$)/.test(source) && !/[?&]type=style(?:&|$)/.test(source),
	extract: ({ content }: { content: string }): string[] => {
		const withoutStyle = content.replace(/<style[\s\S]*?<\/style>/g, '');
		return [...new Set(withoutStyle.match(/[^\s"'`{}<>=]+/g) ?? [])];
	}
};

// Tauri の dev 実行時に固定ポートを使う
export default defineConfig({
	plugins: [
		masterCSS({ mode: 'static', scanner: { adapters: [svelteFullTextAdapter] } }),
		// Tauri から読み込む SPA 構成（SSR なし）
		sveltekit({ adapter: adapter({ fallback: 'index.html' }) })
	],
	clearScreen: false,
	server: { port: 5173, strictPort: true }
});
