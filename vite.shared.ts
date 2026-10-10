import masterCSS from '@master/css.vite';

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

/** アプリ本体と表示拡張（iframe の UI）で共有する Master CSS の設定（vite.config.ts・vite.ext.config.ts が使う） */
export function masterCssPlugin() {
	return masterCSS({ mode: 'static', scanner: { adapters: [svelteFullTextAdapter] } });
}
