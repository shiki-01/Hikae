import { describe, it, expect } from 'vitest';
import { parseOptionsMessage, parseRenderMessage, parseRendererEvent } from './renderer-protocol';

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

describe('parseRenderMessage（比較の表示）', () => {
	const options = { viewMode: 'split', activeBlock: 0 };
	const side = (label: string, state = 'ok') => ({
		label,
		file: { name: 'a.txt', ext: 'txt', state }
	});
	const base = {
		v: 1,
		type: 'render',
		id: 4,
		mode: 'compare',
		left: side('過去'),
		right: side('今'),
		options
	};

	it('2 時点の形が合うものを受理する（差分の有無は問わない）', () => {
		expect(parseRenderMessage(base)).toEqual(base);
		const diff = { kind: 'lines', rows: [] };
		expect(parseRenderMessage({ ...base, diff })).toEqual({ ...base, diff });
		expect(
			parseRenderMessage({ ...base, diff: { kind: 'unavailable', reason: 'binary' } })
		).not.toBeNull();
	});

	it('差分の種類が未知、options が無い・形が違うものは拒否する', () => {
		expect(parseRenderMessage({ ...base, diff: { kind: 'html' } })).toBeNull();
		expect(parseRenderMessage({ ...base, options: undefined })).toBeNull();
		expect(
			parseRenderMessage({ ...base, options: { viewMode: 'grid', activeBlock: null } })
		).toBeNull();
		expect(
			parseRenderMessage({ ...base, options: { viewMode: 'split', activeBlock: -1 } })
		).toBeNull();
		expect(parseRenderMessage({ ...base, left: undefined })).toBeNull();
		expect(parseRenderMessage({ ...base, right: { label: '今' } })).toBeNull();
	});

	it('状態が未知の側は拒否する', () => {
		expect(parseRenderMessage({ ...base, left: side('過去', 'broken') })).toBeNull();
	});
});

describe('parseOptionsMessage（ホスト → iframe の表示の切り替え）', () => {
	it('表示方法と箇所の形が合うものを受理する', () => {
		const message = {
			v: 1,
			type: 'options',
			id: 4,
			options: { viewMode: 'unified', activeBlock: null }
		};
		expect(parseOptionsMessage(message)).toEqual(message);
	});

	it('版・種類・番号・形が合わないものは無視する', () => {
		const options = { viewMode: 'split', activeBlock: 2 };
		expect(parseOptionsMessage({ v: 2, type: 'options', id: 4, options })).toBeNull();
		expect(parseOptionsMessage({ v: 1, type: 'render', id: 4, options })).toBeNull();
		expect(parseOptionsMessage({ v: 1, type: 'options', id: '4', options })).toBeNull();
		expect(
			parseOptionsMessage({
				v: 1,
				type: 'options',
				id: 4,
				options: { viewMode: 'split', activeBlock: '2' }
			})
		).toBeNull();
		expect(parseOptionsMessage(null)).toBeNull();
	});
});
