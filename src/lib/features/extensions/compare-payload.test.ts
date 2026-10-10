import { describe, it, expect } from 'vitest';
import type { FileDiff, FilePreview, OfficeDiff } from '#lib/api/types.js';
import {
	buildCompareMessage,
	compareModelOf,
	diffStatusOf,
	infoOf,
	payloadOfOffice,
	payloadOfText,
	sideOfMedia,
	type CompareInput,
	type CompareModel,
	type ReadyModel,
	type Settled
} from './compare-payload';

/** 取得の結果が無い（使わない入力の既定） */
const none = <T>(): Settled<T> => ({ pending: false, failed: false, data: undefined });
/** 取得が終わって値がある */
const done = <T>(data: T): Settled<T> => ({ pending: false, failed: false, data });
/** 取得の途中 */
const waiting = <T>(): Settled<T> => ({ pending: true, failed: false, data: undefined });
/** 取得に失敗した */
const broken = <T>(): Settled<T> => ({ pending: false, failed: true, data: undefined });

function input(over: Partial<CompareInput>): CompareInput {
	return {
		kind: 'other',
		path: 'dir/メモ.txt',
		fromLabel: '過去',
		toLabel: '今',
		media: { from: none(), to: none() },
		text: none(),
		office: none(),
		info: none(),
		from: none(),
		to: none(),
		...over
	};
}

function ready(model: CompareModel): ReadyModel {
	if (model.state !== 'ready') throw new Error(`ready ではない: ${model.state}`);
	return model;
}

describe('payloadOfText', () => {
	it('行差分・注記・サイズ超えを、表示の差分の形にする', () => {
		const rows = [{ kind: 'add' as const, oldNo: null, newNo: 1, text: 'a' }];
		expect(payloadOfText({ kind: 'text', rows })).toEqual({ kind: 'lines', rows });
		expect(payloadOfText({ kind: 'identical' })).toEqual({ kind: 'identical' });
		expect(payloadOfText({ kind: 'new_empty' })).toEqual({ kind: 'new_empty' });
		expect(payloadOfText({ kind: 'too_large', sizeBytes: 9 })).toEqual({
			kind: 'too_large',
			sizeBytes: 9
		});
	});

	it('内容を比べられない（情報のみ）は、理由 binary とファイルの情報にする', () => {
		const modifiedAt = new Date('2026-10-01T10:00:00Z');
		const diff: FileDiff = { kind: 'info', fileKind: 'unknown', sizeBytes: 12, modifiedAt };
		expect(payloadOfText(diff)).toEqual({
			kind: 'unavailable',
			reason: 'binary',
			info: { fileKind: 'unknown', sizeBytes: 12, modifiedAt: modifiedAt.getTime() }
		});
		expect(infoOf({ kind: 'info', fileKind: 'document', sizeBytes: 1, modifiedAt: null })).toEqual({
			fileKind: 'document',
			sizeBytes: 1,
			modifiedAt: null
		});
		expect(infoOf({ kind: 'identical' })).toBeUndefined();
	});
});

describe('payloadOfOffice', () => {
	it('docx は段落、xlsx はセルの変更とシートの追加・削除にする', () => {
		const docx: OfficeDiff = { kind: 'docx', rows: [], truncated: 2 };
		expect(payloadOfOffice(docx, undefined)).toEqual({
			kind: 'paragraphs',
			rows: [],
			truncated: 2
		});
		const xlsx: OfficeDiff = {
			kind: 'xlsx',
			addedSheets: ['新'],
			removedSheets: [],
			cells: [],
			truncated: 0
		};
		expect(payloadOfOffice(xlsx, undefined)).toEqual({
			kind: 'cells',
			addedSheets: ['新'],
			removedSheets: [],
			cells: [],
			truncated: 0
		});
	});

	it('比べられなかったときは、理由と（あれば）ファイルの情報を渡す', () => {
		const info = { fileKind: 'document' as const, sizeBytes: 5, modifiedAt: null };
		expect(payloadOfOffice({ kind: 'unavailable', reason: 'corrupt' }, info)).toEqual({
			kind: 'unavailable',
			reason: 'corrupt',
			info
		});
	});
});

describe('sideOfMedia', () => {
	it('中身は拡張子から mime を決めて、バイト列として持つ。404 は missing、その他の失敗は unavailable', () => {
		const data = new ArrayBuffer(4);
		expect(sideOfMedia('png', { status: 'ok', data })).toEqual({
			state: 'ok',
			content: { kind: 'bytes', mime: 'image/png', data }
		});
		expect(sideOfMedia('png', { status: 'absent' })).toEqual({ state: 'missing' });
		expect(sideOfMedia('png', { status: 'error' })).toEqual({
			state: 'unavailable',
			reason: 'failed'
		});
	});
});

describe('compareModelOf', () => {
	it('テキストは取得の途中・失敗・結果の順に決まる（両側は内容を持たない）', () => {
		expect(compareModelOf(input({ text: waiting() }))).toEqual({ state: 'pending' });
		expect(compareModelOf(input({ text: broken() }))).toEqual({ state: 'failed' });
		const model = ready(compareModelOf(input({ text: done({ kind: 'identical' }) })));
		expect(model.diff).toEqual({ kind: 'identical' });
		expect(model.left).toEqual({
			label: '過去',
			file: { name: 'メモ.txt', ext: 'txt', state: 'ok' }
		});
		expect(model.right.label).toBe('今');
	});

	it('画像は両側の取得が終わるまで保留し、404 の側を missing にする（差分は無い）', () => {
		expect(
			compareModelOf(
				input({ kind: 'image', path: 'a.png', media: { from: waiting(), to: none() } })
			)
		).toEqual({ state: 'pending' });
		const model = ready(
			compareModelOf(
				input({
					kind: 'image',
					path: 'a.png',
					media: {
						from: done({ status: 'absent' }),
						to: done({ status: 'ok', data: new ArrayBuffer(1) })
					}
				})
			)
		);
		expect(model.diff).toBeUndefined();
		expect(model.left.file.state).toBe('missing');
		expect(model.right.file.state).toBe('ok');
		expect(model.right.content).toMatchObject({ kind: 'bytes', mime: 'image/png' });
	});

	it('docx は比べられないとき、テキストの情報が届くまで待つ', () => {
		const unavailable: OfficeDiff = { kind: 'unavailable', reason: 'too_large' };
		expect(
			compareModelOf(
				input({ kind: 'docx', path: 'a.docx', office: done(unavailable), info: waiting() })
			)
		).toEqual({ state: 'pending' });
		const model = ready(
			compareModelOf(
				input({
					kind: 'docx',
					path: 'a.docx',
					office: done(unavailable),
					info: done({ kind: 'info', fileKind: 'document', sizeBytes: 3, modifiedAt: null })
				})
			)
		);
		expect(model.diff).toEqual({
			kind: 'unavailable',
			reason: 'too_large',
			info: { fileKind: 'document', sizeBytes: 3, modifiedAt: null }
		});
	});

	it('xlsx は片側が無いとき、その側を missing にし、今の版のシートを内容として渡す', () => {
		const sheets = [{ name: 'Sheet1', rows: 1, cols: 1, cells: [], truncated: false }];
		const model = ready(
			compareModelOf(
				input({
					kind: 'xlsx',
					path: 'b.xlsx',
					office: done({
						kind: 'xlsx',
						addedSheets: [],
						removedSheets: [],
						cells: [],
						truncated: 0
					}),
					from: done({ kind: 'not_found' } satisfies FilePreview),
					to: done({ kind: 'xlsx', sheets })
				})
			)
		);
		expect(model.left.file.state).toBe('missing');
		expect(model.right.content).toEqual({ kind: 'cells', sheets });
		expect(model.diff?.kind).toBe('cells');
	});

	it('xlsx は読めない側の理由を優先して、比べられなかった表示にする', () => {
		const model = ready(
			compareModelOf(
				input({
					kind: 'xlsx',
					path: 'b.xlsx',
					office: done({ kind: 'unavailable', reason: 'corrupt' }),
					from: done({ kind: 'xlsx', sheets: [] }),
					to: done({ kind: 'unavailable', reason: 'binary' })
				})
			)
		);
		expect(model.diff).toEqual({ kind: 'unavailable', reason: 'binary' });
		expect(model.right.file).toMatchObject({ state: 'unavailable', reason: 'binary' });
	});

	it('pdf と pptx は比較の表示を使わない（failed）', () => {
		expect(compareModelOf(input({ kind: 'pdf' }))).toEqual({ state: 'failed' });
		expect(compareModelOf(input({ kind: 'pptx' }))).toEqual({ state: 'failed' });
	});
});

describe('buildCompareMessage', () => {
	it('画像の中身は複製して転送し、元のバッファは残す。パスとプロジェクトの ID は含めない', () => {
		const data = new Uint8Array([1, 2, 3]).buffer;
		const model = ready(
			compareModelOf(
				input({
					kind: 'image',
					path: 'dir/a.png',
					media: { from: done({ status: 'ok', data }), to: done({ status: 'absent' }) }
				})
			)
		);
		const { message, transfer } = buildCompareMessage(9, model, {
			viewMode: 'unified',
			activeBlock: null
		});
		expect(message).toMatchObject({
			v: 1,
			type: 'render',
			id: 9,
			mode: 'compare',
			options: { viewMode: 'unified', activeBlock: null }
		});
		expect(message.diff).toBeUndefined();
		expect(transfer).toHaveLength(1);
		expect(transfer[0]).not.toBe(data);
		expect(data.byteLength).toBe(3);
		expect(JSON.stringify(message)).not.toContain('dir/');
	});

	it('差分の結果は、そのまま渡す', () => {
		const model = ready(compareModelOf(input({ text: done({ kind: 'text', rows: [] }) })));
		const { message, transfer } = buildCompareMessage(1, model, {
			viewMode: 'split',
			activeBlock: 0
		});
		expect(message.diff).toEqual({ kind: 'lines', rows: [] });
		expect(transfer).toEqual([]);
	});
});

describe('diffStatusOf', () => {
	it('行差分は箇所の数を持つ。注記は前へ・次へを出さない', () => {
		const lines = ready(
			compareModelOf(
				input({
					text: done({
						kind: 'text',
						rows: [
							{ kind: 'del', oldNo: 1, newNo: null, text: 'a' },
							{ kind: 'context', oldNo: 2, newNo: 2, text: 'b' },
							{ kind: 'add', oldNo: null, newNo: 3, text: 'c' }
						]
					})
				})
			)
		);
		expect(diffStatusOf(lines)).toEqual({ textual: true, ready: true, total: 2 });
		expect(
			diffStatusOf(ready(compareModelOf(input({ text: done({ kind: 'identical' }) }))))
		).toEqual({
			textual: true,
			ready: true,
			total: 0
		});
		expect(diffStatusOf({ state: 'pending' })).toEqual({ textual: false, ready: false, total: 0 });
		expect(diffStatusOf({ state: 'failed' })).toEqual({ textual: false, ready: true, total: 0 });
	});
});
