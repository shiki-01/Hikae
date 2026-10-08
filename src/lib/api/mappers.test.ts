import { describe, it, expect } from 'vitest';
import { describeError } from '#lib/features/notifications/error-view.js';
import {
	fromUnixSeconds,
	mapChange,
	mapConflict,
	mapConflictKind,
	mapDiff,
	mapFileEntries,
	mapError,
	mapImpact,
	mapPull,
	mapResolutions,
	mapSavePoint,
	parseDate
} from './mappers';

describe('日時の変換', () => {
	it('ISO 8601 を Date にする', () => {
		expect(parseDate('2026-01-02T03:04:05+09:00')?.toISOString()).toBe('2026-01-01T18:04:05.000Z');
	});

	it('解釈できない文字列は null', () => {
		expect(parseDate('not a date')).toBeNull();
	});

	it('履歴の自動保存と手動保存を区別する', () => {
		const base = { commit: 'abc', timestamp: '2026-01-02T03:04:05Z', message: 'm' };
		const manual = mapSavePoint({
			...base,
			changed_files_count: 1,
			is_snapshot: false,
			pc_name: null,
			cloud_synced: false
		});
		const auto = mapSavePoint({
			...base,
			changed_files_count: 1,
			is_snapshot: true,
			pc_name: null,
			cloud_synced: false
		});
		expect(manual).toMatchObject({ id: 'abc', kind: 'save', message: 'm' });
		expect(manual.createdAt.toISOString()).toBe('2026-01-02T03:04:05.000Z');
		expect(auto.kind).toBe('auto');
	});
});

describe('変更種別の変換', () => {
	it('種別を保ち、競合中のファイルに印を付ける', () => {
		const conflicts = new Set(['b.txt']);
		expect(mapChange({ path: 'a.txt', kind: 'added', conflicted: false }, conflicts)).toEqual({
			id: 'a.txt',
			path: 'a.txt',
			type: 'added',
			isConflict: false
		});
		expect(
			mapChange({ path: 'b.txt', kind: 'modified', conflicted: false }, conflicts).isConflict
		).toBe(true);
		expect(
			mapChange({ path: 'c.txt', kind: 'modified', conflicted: true }, conflicts).isConflict
		).toBe(true);
	});

	it('復元の影響を 3 種に振り分ける', () => {
		expect(mapImpact({ modified: ['m'], deleted: ['d'], created: ['c'] })).toEqual([
			{ path: 'm', type: 'modified' },
			{ path: 'd', type: 'removed' },
			{ path: 'c', type: 'restored' }
		]);
	});
});

describe('競合種別の変換', () => {
	it('自分側の削除は「この PC で削除」、相手側の削除は「クラウドで削除」', () => {
		expect(mapConflictKind('deleted-by-us')).toBe('deleted_on_this_pc');
		expect(mapConflictKind('deleted-by-them')).toBe('deleted_in_cloud');
		expect(mapConflictKind('both-modified')).toBe('both');
		expect(mapConflictKind('both-added')).toBe('both');
		expect(mapConflictKind('both-deleted')).toBe('both');
	});

	it('競合ファイルの変換でパスと種別が保たれる', () => {
		expect(
			mapConflict({
				path: 'x',
				kind: 'both-modified',
				this_saved_at: null,
				cloud_saved_at: null,
				this_pc_name: null,
				cloud_pc_name: null
			})
		).toMatchObject({
			path: 'x',
			kind: 'both'
		});
	});

	it('解消の選択をバックエンドの形式に変換する', () => {
		expect(
			mapResolutions([
				{ path: 'a', choice: 'this', keepBoth: false },
				{ path: 'b', choice: 'cloud', keepBoth: true }
			])
		).toEqual({
			choices: [
				['a', 'mine'],
				['b', 'theirs']
			],
			keepOtherCopy: true
		});
	});

	it('取り込み結果から競合数を取り出す', () => {
		expect(
			mapPull({ outcome: 'up-to-date', conflicts: [], size_check: null, unsaved_count: null })
		).toEqual({
			mergedCount: 0,
			conflictCount: 0,
			sizeCheck: null,
			unsavedCount: null
		});
		expect(
			mapPull({
				outcome: 'conflicted',
				conflicts: [
					{
						path: 'a',
						kind: 'both-added',
						this_saved_at: null,
						cloud_saved_at: null,
						this_pc_name: null,
						cloud_pc_name: null
					}
				],
				size_check: null,
				unsaved_count: null
			})
		).toEqual({ mergedCount: 0, conflictCount: 1, sizeCheck: null, unsavedCount: null });
		expect(
			mapPull({ outcome: 'merged', conflicts: [], size_check: null, unsaved_count: null })
				.mergedCount
		).toBe(1);
	});
});

describe('差分の変換', () => {
	it('空なら同一', () => {
		expect(mapDiff([])).toEqual({ kind: 'identical' });
	});

	it('旧・新の行番号を振る', () => {
		const diff = mapDiff([
			{ kind: 'context', content: 'a' },
			{ kind: 'removed', content: 'b' },
			{ kind: 'added', content: 'c' },
			{ kind: 'context', content: 'd' }
		]);
		expect(diff).toEqual({
			kind: 'text',
			rows: [
				{ kind: 'context', oldNo: 1, newNo: 1, text: 'a' },
				{ kind: 'del', oldNo: 2, newNo: null, text: 'b' },
				{ kind: 'add', oldNo: null, newNo: 2, text: 'c' },
				{ kind: 'context', oldNo: 3, newNo: 3, text: 'd' }
			]
		});
	});
});

describe('エラーの 3 要素の変換', () => {
	const backend = {
		code: 'test_unknown_code',
		params: [] as [string, string][],
		what_happened: '保存に失敗しました',
		data_is_safe: 'ファイルは無事です',
		next_action: 'もう一度試してください',
		technical_info: 'git: boom'
	};

	it('3 要素と技術情報を保持する', () => {
		const error = mapError(backend);
		expect(error.code).toBe('backend');
		expect(error.backend).toEqual({
			code: 'test_unknown_code',
			params: {},
			whatHappened: '保存に失敗しました',
			dataIsSafe: 'ファイルは無事です',
			nextAction: 'もう一度試してください'
		});
		expect(error.technical).toBe('git: boom');
	});

	it('技術情報が無ければ空文字', () => {
		expect(mapError({ ...backend, technical_info: null }).technical).toBe('');
	});

	it('ダイアログ用の表示に 3 要素がそのまま載る', () => {
		const view = describeError(mapError(backend));
		expect(view.title).toBe('保存に失敗しました');
		expect(view.message).toBe('ファイルは無事です\nもう一度試してください');
		expect(view.autoRecovering).toBe(false);
		expect(view.technical).toBe('git: boom');
	});
});

describe('競合の日時', () => {
	it('Unix 秒を Date にし、無い場合は null にする', () => {
		const base = {
			path: 'a.txt',
			kind: 'both-modified' as const,
			this_pc_name: null,
			cloud_pc_name: null
		};
		const both = mapConflict({
			...base,
			this_saved_at: 1_700_000_000,
			cloud_saved_at: 1_700_000_060
		});
		expect(both.thisPcSavedAt?.getTime()).toBe(1_700_000_000_000);
		expect(both.cloudSavedAt?.getTime()).toBe(1_700_000_060_000);
		expect(both.cloudPcName).toBeNull();

		const none = mapConflict({ ...base, this_saved_at: null, cloud_saved_at: null });
		expect(none.thisPcSavedAt).toBeNull();
		expect(none.cloudSavedAt).toBeNull();
	});

	it('0 以下や非有限値は日時なしとして扱う', () => {
		expect(fromUnixSeconds(0)).toBeNull();
		expect(fromUnixSeconds(-1)).toBeNull();
		expect(fromUnixSeconds(Number.NaN)).toBeNull();
	});
});

describe('保存時点のファイル一覧の変換', () => {
	it('区切りを / に統一し、パス順に並べる', () => {
		const result = mapFileEntries([
			{ path: 'b\\c.txt', size: 5 },
			{ path: 'a.txt', size: null }
		]);
		expect(result).toEqual([
			{ path: 'a.txt', size: null },
			{ path: 'b/c.txt', size: 5 }
		]);
	});
});
