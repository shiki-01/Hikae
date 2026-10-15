// クリップボードの読み取り（navigator.clipboard）。失敗したら呼び出し側が貼り付け欄へ切り替える。

export type ClipboardContent =
	| { kind: 'image'; blob: Blob; mime: string }
	| { kind: 'text'; text: string }
	| { kind: 'empty' }
	| { kind: 'unavailable' };

/** 読み取れた型の一覧から、画像の型を選ぶ（png を優先。画像が無ければ null） */
export function imageTypeOf(types: readonly string[]): string | null {
	const images = types.filter((type) => type.startsWith('image/'));
	if (images.length === 0) return null;
	return images.includes('image/png') ? 'image/png' : images[0];
}

/** 画像を優先し、無ければ文字列を読む。読めない・権限が無いときは unavailable */
export async function readClipboard(): Promise<ClipboardContent> {
	const clipboard = globalThis.navigator?.clipboard;
	if (!clipboard || typeof clipboard.read !== 'function') return { kind: 'unavailable' };
	try {
		const items = await clipboard.read();
		for (const item of items) {
			const type = imageTypeOf(item.types);
			if (type !== null) return { kind: 'image', blob: await item.getType(type), mime: type };
		}
		const text = await clipboard.readText();
		return text.trim() === '' ? { kind: 'empty' } : { kind: 'text', text };
	} catch {
		return { kind: 'unavailable' };
	}
}
