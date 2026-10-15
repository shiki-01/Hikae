// Map / WeakMap の `getOrInsert` と `getOrInsertComputed`（ES2026）の代替。
// pdf.js 6 はこれを使うが、古い WebView（iOS の Safari、Android System WebView、macOS の WKWebView）には無く、
// 無いと PDF のページの描画が `getOrInsertComputed is not a function` で失敗する。
// pdf.js より先に評価される必要があるため、`pdf-document.ts` の最初の import にする。

type Upsertable<K, V> = {
	has(key: K): boolean;
	get(key: K): V | undefined;
	set(key: K, value: V): unknown;
	getOrInsert?: (key: K, value: V) => V;
	getOrInsertComputed?: (key: K, callback: (key: K) => V) => V;
};

export function installUpsertPolyfill(): void {
	for (const ctor of [Map, WeakMap] as unknown as { prototype: Upsertable<unknown, unknown> }[]) {
		const proto = ctor.prototype;
		if (typeof proto.getOrInsert !== 'function') {
			Object.defineProperty(proto, 'getOrInsert', {
				value(this: Upsertable<unknown, unknown>, key: unknown, value: unknown) {
					if (this.has(key)) return this.get(key);
					this.set(key, value);
					return value;
				},
				writable: true,
				configurable: true
			});
		}
		if (typeof proto.getOrInsertComputed !== 'function') {
			Object.defineProperty(proto, 'getOrInsertComputed', {
				value(
					this: Upsertable<unknown, unknown>,
					key: unknown,
					callback: (key: unknown) => unknown
				) {
					if (this.has(key)) return this.get(key);
					const value = callback(key);
					this.set(key, value);
					return value;
				},
				writable: true,
				configurable: true
			});
		}
	}
}

installUpsertPolyfill();
