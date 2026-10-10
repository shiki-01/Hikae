import { describe, it, expect } from 'vitest';
import { parseRenderMessage, parseRendererEvent } from './renderer-protocol';

describe('parseRendererEvent（iframe → ホスト）', () => {
	it('既知の種類で、型の合うものだけを受理する', () => {
		expect(parseRendererEvent({ v: 1, type: 'ready' })).toEqual({ v: 1, type: 'ready' });
		expect(parseRendererEvent({ v: 1, type: 'open' })).toEqual({ v: 1, type: 'open' });
		expect(parseRendererEvent({ v: 1, type: 'error', id: 3, code: 'render-failed' })).toEqual({
			v: 1,
			type: 'error',
			id: 3,
			code: 'render-failed'
		});
		expect(parseRendererEvent({ v: 1, type: 'error', id: 0, code: 'unsupported' })).toEqual({
			v: 1,
			type: 'error',
			id: 0,
			code: 'unsupported'
		});
	});

	it('版が違う、または v が無いものは無視する', () => {
		expect(parseRendererEvent({ v: 2, type: 'ready' })).toBeNull();
		expect(parseRendererEvent({ type: 'ready' })).toBeNull();
		expect(parseRendererEvent({ v: '1', type: 'ready' })).toBeNull();
	});

	it('未知の種類は無視する（送信の要求を増やさない）', () => {
		expect(parseRendererEvent({ v: 1, type: 'navigate', url: 'https://example.com' })).toBeNull();
		expect(parseRendererEvent({ v: 1, type: 'render' })).toBeNull();
	});

	it('error の id と code は型と値の範囲を確かめる', () => {
		for (const id of ['1', -1, 1.5, Number.NaN, null, undefined]) {
			expect(parseRendererEvent({ v: 1, type: 'error', id, code: 'render-failed' })).toBeNull();
		}
		for (const code of ['crash', 'RENDER-FAILED', 1, undefined]) {
			expect(parseRendererEvent({ v: 1, type: 'error', id: 1, code })).toBeNull();
		}
	});

	it('オブジェクトでない値は無視する', () => {
		expect(parseRendererEvent(null)).toBeNull();
		expect(parseRendererEvent(undefined)).toBeNull();
		expect(parseRendererEvent('ready')).toBeNull();
		expect(parseRendererEvent(1)).toBeNull();
	});
});

describe('parseRenderMessage（ホスト → iframe）', () => {
	const base = { v: 1, type: 'render', id: 7, mode: 'view' };

	it('表示の対象の形が合うものを受理する', () => {
		const ok = { ...base, file: { name: 'a.txt', ext: 'txt', state: 'ok' } };
		expect(parseRenderMessage(ok)).toEqual(ok);
		const missing = { ...base, file: { name: 'a.txt', ext: 'txt', state: 'missing' } };
		expect(parseRenderMessage(missing)).toEqual(missing);
	});

	it('mode が view 以外、状態が未知、ファイル情報が無いものは拒否する', () => {
		expect(
			parseRenderMessage({ ...base, mode: 'compare', file: { name: 'a', ext: '', state: 'ok' } })
		).toBeNull();
		expect(
			parseRenderMessage({ ...base, file: { name: 'a', ext: '', state: 'broken' } })
		).toBeNull();
		expect(parseRenderMessage({ ...base })).toBeNull();
		expect(
			parseRenderMessage({ ...base, id: '7', file: { name: 'a', ext: '', state: 'ok' } })
		).toBeNull();
	});

	it('ready など別の種類は受理しない', () => {
		expect(parseRenderMessage({ v: 1, type: 'ready' })).toBeNull();
	});
});
