import { describe, expect, it } from 'vitest';
import type { UploadPlan } from '#lib/api/mobile-types.js';
import {
	applyPlan,
	conflictItems,
	createItem,
	hasSaveTargets,
	nameProblem,
	nextRunState,
	planInputs,
	renameItem,
	setOnExists,
	uploadInputs,
	type UploadItem
} from './upload-items';

function item(id: string, name: string, extra: Partial<UploadItem> = {}): UploadItem {
	return {
		...createItem({ id, source: 'file', name, size: 3, contentBase64: 'AAAA' }),
		...extra
	};
}

describe('アップロード項目', () => {
	it('既定は「両方残す」で、同名の判定は無い', () => {
		const created = createItem({
			id: '1',
			source: 'text',
			name: 'a.txt',
			size: 1,
			contentBase64: 'AA'
		});
		expect(created.onExists).toBe('keepBoth');
		expect(created.exists).toBe(false);
	});

	it('名前を変えると、同名の判定を取り消す（確認の結果は古くなる）', () => {
		const items = [item('1', 'a.txt', { exists: true })];
		expect(renameItem(items, '1', 'b.txt')[0]).toMatchObject({ name: 'b.txt', exists: false });
	});

	it('同名のときの扱いを選んでも、確認の結果は変わらない', () => {
		const items = [item('1', 'a.txt', { exists: true })];
		expect(setOnExists(items, '1', 'replace')[0]).toMatchObject({
			onExists: 'replace',
			exists: true
		});
	});

	it('確認の結果は、入力と同じ順序で項目に入る', () => {
		const items = [item('1', 'a.txt'), item('2', 'b.txt')];
		const plan: UploadPlan = {
			branch: 'main',
			items: [
				{ targetPath: 'a.txt', exists: false },
				{ targetPath: 'b.txt', exists: true }
			]
		};
		const applied = applyPlan(items, plan);
		expect(applied.map((value) => value.exists)).toEqual([false, true]);
		expect(conflictItems(applied).map((value) => value.id)).toEqual(['2']);
	});

	it('入力は、追加先と名前・サイズ・同名の扱いに変換する', () => {
		const items = [item('1', 'a.txt', { onExists: 'skip' })];
		expect(planInputs(items, 'docs')).toEqual([{ targetDir: 'docs', name: 'a.txt', size: 3 }]);
		expect(uploadInputs(items, 'docs')).toEqual([
			{ targetDir: 'docs', name: 'a.txt', contentBase64: 'AAAA', onExists: 'skip' }
		]);
	});

	it('全部が「追加しない」の同名だけなら、保存する項目は無い', () => {
		expect(hasSaveTargets([item('1', 'a.txt', { exists: true, onExists: 'skip' })])).toBe(false);
		expect(
			hasSaveTargets([item('1', 'a.txt', { exists: true, onExists: 'skip' }), item('2', 'b.txt')])
		).toBe(true);
	});
});

describe('名前の検査', () => {
	const items = [item('1', 'A.txt'), item('2', 'b.txt')];

	it('空（空白だけを含む）は empty', () => {
		expect(nameProblem('', items, '1')).toBe('empty');
		expect(nameProblem('   ', items, '1')).toBe('empty');
	});

	it('区切り文字・制御文字・「.」「..」は invalid', () => {
		expect(nameProblem('a/b.txt', items, '1')).toBe('invalid');
		expect(nameProblem('a\\b.txt', items, '1')).toBe('invalid');
		expect(nameProblem('a\u0001.txt', items, '1')).toBe('invalid');
		expect(nameProblem('..', items, '1')).toBe('invalid');
	});

	it('ほかの項目と同じ名前（大文字と小文字を区別しない）は duplicate。自分自身は除く', () => {
		expect(nameProblem('a.txt', items, '2')).toBe('duplicate');
		expect(nameProblem('a.txt', items, '1')).toBeNull();
		expect(nameProblem('c.txt', items, '1')).toBeNull();
	});
});

describe('実行の状態遷移', () => {
	it('編集中 → 同名の確認済み → 保存中 → 完了', () => {
		let state = nextRunState('editing', 'planned_conflict');
		expect(state).toBe('checked');
		state = nextRunState(state, 'start');
		expect(state).toBe('uploading');
		state = nextRunState(state, 'succeeded');
		expect(state).toBe('done');
	});

	it('同名がなければ確認の後すぐに保存へ進む', () => {
		expect(nextRunState('editing', 'planned_clear')).toBe('uploading');
	});

	it('確認済みのあとに編集すると、編集中へ戻る（確認は古くなる）', () => {
		expect(nextRunState('checked', 'edit')).toBe('editing');
	});

	it('保存に失敗したら編集中へ戻す（項目は残る）', () => {
		expect(nextRunState('uploading', 'failed')).toBe('editing');
	});

	it('保存中の編集は受け付けない（状態は変わらない）', () => {
		expect(nextRunState('uploading', 'edit')).toBe('uploading');
		expect(nextRunState('editing', 'succeeded')).toBe('editing');
	});

	it('完了のあとの「次の項目」は編集中へ戻る', () => {
		expect(nextRunState('done', 'reset')).toBe('editing');
	});
});
