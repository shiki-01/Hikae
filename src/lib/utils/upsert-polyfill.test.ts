import { describe, expect, it } from 'vitest';
import { installUpsertPolyfill } from './upsert-polyfill';

type WithUpsert<K, V> = Map<K, V> & {
	getOrInsert: (key: K, value: V) => V;
	getOrInsertComputed: (key: K, callback: (key: K) => V) => V;
};

describe('Map の getOrInsert の代替', () => {
	installUpsertPolyfill();

	it('getOrInsert: 無いときだけ入れる', () => {
		const map = new Map<string, number>() as WithUpsert<string, number>;
		expect(map.getOrInsert('a', 1)).toBe(1);
		expect(map.getOrInsert('a', 2)).toBe(1);
		expect(map.get('a')).toBe(1);
	});

	it('getOrInsertComputed: 無いときだけ計算し、キーを渡す', () => {
		const map = new Map<string, string>() as WithUpsert<string, string>;
		let calls = 0;
		expect(
			map.getOrInsertComputed('k', (key) => {
				calls++;
				return key + '!';
			})
		).toBe('k!');
		expect(map.getOrInsertComputed('k', () => 'other')).toBe('k!');
		expect(calls).toBe(1);
	});

	it('undefined が入っているキーも「ある」と見なす', () => {
		const map = new Map<string, number | undefined>() as WithUpsert<string, number | undefined>;
		map.set('u', undefined);
		expect(map.getOrInsert('u', 5)).toBeUndefined();
	});

	it('WeakMap にも入る', () => {
		const key = {};
		const map = new WeakMap<object, number>() as unknown as {
			getOrInsertComputed: (k: object, cb: (k: object) => number) => number;
		};
		expect(map.getOrInsertComputed(key, () => 7)).toBe(7);
		expect(map.getOrInsertComputed(key, () => 9)).toBe(7);
	});
});
