import { describe, it, expect } from 'vitest';
import ja from '#lib/i18n/ja.js';
import { customGgufMessageKey, scopeHintKey, stageLevelKey, stageNoteKey } from './model-view';

describe('段階の表示（軽量・標準）', () => {
	it('段階 1 は軽量、段階 2 以上は標準', () => {
		expect(stageLevelKey(1)).toBe('ai.level_light');
		expect(stageLevelKey(2)).toBe('ai.level_standard');
		expect(stageLevelKey(3)).toBe('ai.level_standard');
	});

	it('段階 1 だけ、内容の理解が限定的であることの説明を出す', () => {
		expect(stageNoteKey(1)).toBe('ai.stage_light_note');
		expect(stageNoteKey(2)).toBeNull();
		expect(ja['ai.stage_light_note']).toContain('内容の理解は限定的です');
		expect(ja['ai.stage_light_note']).toContain('「標準」をおすすめします');
	});

	it('段階の文言は、段階の数と名前を両方出す', () => {
		expect(ja['ai.stage']).toBe('段階 {stage}（{level}）');
	});
});

describe('AI に渡す範囲の注意', () => {
	it('段階 1 のモデルで「テキスト差分まで」のときだけ、ファイル名まで を勧める', () => {
		expect(scopeHintKey(1, 'text-diff')).toBe('ai.scope_light_hint');
		expect(scopeHintKey(1, 'file-names')).toBeNull();
		expect(scopeHintKey(2, 'text-diff')).toBeNull();
		// カタログのモデルではない（カスタム・Ollama など）ときは出さない
		expect(scopeHintKey(null, 'text-diff')).toBeNull();
	});

	it('注意は「勧める」だけで、範囲の選択を妨げない文言にしている', () => {
		expect(ja['ai.scope_light_hint']).toContain('「ファイル名まで」を選ぶと');
		expect(ja['ai.scope_light_hint']).not.toMatch(/選べません|使えません/);
	});
});

describe('カスタム GGUF の検査の理由', () => {
	it('理由ごとに文言キーがあり、キーは日本語の文言になっている', () => {
		const reasons = [
			'empty',
			'control-char',
			'not-found',
			'link',
			'not-a-file',
			'wrong-extension',
			'too-large',
			'not-gguf',
			'unreadable'
		] as const;
		for (const reason of reasons) {
			const key = customGgufMessageKey(reason);
			expect(key).toBe(`ai.custom_reason.${reason}`);
			expect(ja[key]).toBeTruthy();
		}
	});

	it('注意書きは、SHA-256 と利用者の責任を明記する', () => {
		expect(ja['ai.custom_caution']).toBe(
			'SHA-256 は検証されません。モデルの内容とライセンスはご自身の責任で確認してください。'
		);
	});

	it('入手元とカスタムのパスの文言がある', () => {
		expect(ja['ai.source']).toBeTruthy();
		expect(ja['ai.source_custom']).toBeTruthy();
		expect(ja['ai.custom_path']).toBeTruthy();
		expect(ja['ai.custom_pick']).toBe('選ぶ');
		expect(ja['ai.reason.custom-invalid']).toBeTruthy();
	});
});
