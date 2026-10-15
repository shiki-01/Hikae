// バイト列と base64 の変換。大きなファイルでも落ちないよう、`String.fromCharCode` へ渡す量を分けて処理する
// （一度に展開すると引数の数が上限（スタック）を超える）。

/** 1 回の `String.fromCharCode` に渡すバイト数 */
const CHUNK_BYTES = 0x8000;

export function bytesToBase64(bytes: Uint8Array): string {
	const parts: string[] = [];
	for (let offset = 0; offset < bytes.length; offset += CHUNK_BYTES) {
		const chunk = bytes.subarray(offset, offset + CHUNK_BYTES);
		parts.push(String.fromCharCode(...chunk));
	}
	return btoa(parts.join(''));
}

export function base64ToBytes(base64: string): Uint8Array {
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return bytes;
}

/** 文字列を UTF-8 として base64 にする（テキストの入力の保存用） */
export function textToBase64(text: string): string {
	return bytesToBase64(new TextEncoder().encode(text));
}

/** File（または Blob）の中身を base64 にする */
export async function fileToBase64(file: Blob): Promise<string> {
	return bytesToBase64(new Uint8Array(await file.arrayBuffer()));
}
