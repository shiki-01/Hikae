import { describe, it, expect } from 'vitest';
import type { FilePreview } from '#lib/api/types.js';
import { buildRenderMessage, mediaMimeOf, outcomeOfPreview } from './render-content';

describe('outcomeOfPreview（読み取りの結果 → 表示の内容）', () => {
	it('テキスト・docx・pptx・xlsx を、表示の種類に写す', () => {
		expect(outcomeOfPreview({ kind: 'text', content: 'あ', truncated: false })).toEqual({
			state: 'ok',
			content: { kind: 'text', text: 'あ', truncated: false }
		});
		expect(outcomeOfPreview({ kind: 'docx', paragraphs: ['一', ''], truncated: true })).toEqual({
			state: 'ok',
			content: { kind: 'paragraphs', paragraphs: ['一', ''], truncated: true }
		});
		const slides = [{ index: 1, paragraphs: ['題'] }];
		expect(outcomeOfPreview({ kind: 'pptx', slides, truncated: false })).toEqual({
			state: 'ok',
			content: { kind: 'pages', pages: slides, truncated: false }
		});
		const sheets = [
			{
				name: 'Sheet1',
				rows: 1,
				cols: 1,
				cells: [{ row: 0, col: 0, value: '1' }],
				truncated: false
			}
		];
		expect(outcomeOfPreview({ kind: 'xlsx', sheets })).toEqual({
			state: 'ok',
			content: { kind: 'cells', sheets }
		});
	});

	it('存在しないものは missing、内容を表示できないものは理由を渡す', () => {
		expect(outcomeOfPreview({ kind: 'not_found' })).toEqual({ state: 'missing' });
		const unavailable: FilePreview = { kind: 'unavailable', reason: 'too_large' };
		expect(outcomeOfPreview(unavailable)).toEqual({ state: 'unavailable', reason: 'too_large' });
	});
});

describe('mediaMimeOf', () => {
	it('画像と pdf の mime を返す。それ以外は null', () => {
		expect(mediaMimeOf('png')).toBe('image/png');
		expect(mediaMimeOf('jpg')).toBe('image/jpeg');
		expect(mediaMimeOf('pdf')).toBe('application/pdf');
		expect(mediaMimeOf('txt')).toBeNull();
		expect(mediaMimeOf('')).toBeNull();
	});
});

describe('buildRenderMessage', () => {
	it('ok の内容は、名前と拡張子だけを file に入れる（パスは入れない）', () => {
		const { message, transfer } = buildRenderMessage(4, 'dir/sub/メモ.TXT', {
			state: 'ok',
			content: { kind: 'text', text: 'x', truncated: false }
		});
		expect(message).toEqual({
			v: 1,
			type: 'render',
			id: 4,
			mode: 'view',
			file: { name: 'メモ.TXT', ext: 'txt', state: 'ok' },
			content: { kind: 'text', text: 'x', truncated: false }
		});
		expect(JSON.stringify(message)).not.toContain('dir/sub');
		expect(transfer).toEqual([]);
	});

	it('bytes は複製して転送する（元のバッファは残る）', () => {
		const original = new Uint8Array([1, 2, 3]).buffer;
		const { message, transfer } = buildRenderMessage(1, 'a.png', {
			state: 'ok',
			content: { kind: 'bytes', mime: 'image/png', data: original }
		});
		expect(message.content?.kind).toBe('bytes');
		expect(transfer).toHaveLength(1);
		expect(transfer[0]).not.toBe(original);
		expect(original.byteLength).toBe(3);
		if (message.content?.kind === 'bytes') {
			expect(message.content.data).toBe(transfer[0]);
			expect(Array.from(new Uint8Array(message.content.data))).toEqual([1, 2, 3]);
		}
	});

	it('missing と unavailable は内容を持たない', () => {
		expect(buildRenderMessage(2, 'a.bin', { state: 'missing' }).message).toEqual({
			v: 1,
			type: 'render',
			id: 2,
			mode: 'view',
			file: { name: 'a.bin', ext: 'bin', state: 'missing' }
		});
		const { message } = buildRenderMessage(3, 'a.bin', { state: 'unavailable', reason: 'binary' });
		expect(message.file).toEqual({
			name: 'a.bin',
			ext: 'bin',
			state: 'unavailable',
			reason: 'binary'
		});
		expect(message.content).toBeUndefined();
	});
});
