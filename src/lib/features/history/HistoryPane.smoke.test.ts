// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import type { SavePoint } from '#lib/api/types.js';
import { buildTimeline } from '#lib/components/timeline.js';
import HistoryPane from './HistoryPane.svelte';

setupSmoke();

// jsdom には IntersectionObserver が無いため、見張りの登録と発火をテストから操作できる代替を使う
type Callback = (records: { isIntersecting: boolean }[]) => void;
let observers: { callback: Callback; disconnected: boolean }[] = [];

class FakeObserver {
	record: { callback: Callback; disconnected: boolean };
	constructor(callback: Callback) {
		this.record = { callback, disconnected: false };
		observers.push(this.record);
	}
	observe() {}
	unobserve() {}
	disconnect() {
		this.record.disconnected = true;
	}
}

beforeEach(() => {
	observers = [];
	vi.stubGlobal('IntersectionObserver', FakeObserver);
});

afterEach(() => vi.unstubAllGlobals());

const points: SavePoint[] = [
	{
		id: 'a',
		createdAt: new Date('2026-10-05T12:14:00'),
		message: 'second',
		kind: 'save',
		cloudSynced: false
	},
	{
		id: 'b',
		createdAt: new Date('2026-10-04T18:02:00'),
		message: 'first',
		kind: 'save',
		cloudSynced: false
	}
];
const entries = buildTimeline(points);

const base = {
	entries,
	loading: false,
	selectedId: null,
	expanded: [],
	onselect: () => {},
	ontoggle: () => {}
};

describe('HistoryPane のスモークテスト（無限スクロール）', () => {
	it('続きがあるときは末尾に読み込み中の表示が出て、末尾が見えたら次を読み込む', () => {
		const onloadmore = vi.fn();
		renderWithClient(HistoryPane, { ...base, hasMore: true, onloadmore });
		expect(screen.getByTestId('history-sentinel')).toBeTruthy();
		expect(screen.getByText('second')).toBeTruthy();
		const active = observers.filter((o) => !o.disconnected);
		expect(active.length).toBe(1);
		active[0].callback([{ isIntersecting: false }]);
		expect(onloadmore).not.toHaveBeenCalled();
		active[0].callback([{ isIntersecting: true }]);
		expect(onloadmore).toHaveBeenCalledTimes(1);
	});

	it('続きが無いときは末尾の表示も見張りも出ない', () => {
		renderWithClient(HistoryPane, { ...base, hasMore: false, onloadmore: vi.fn() });
		expect(screen.queryByTestId('history-sentinel')).toBeNull();
		expect(observers.filter((o) => !o.disconnected)).toEqual([]);
	});

	it('読み込み中は新しい見張りを作らない', () => {
		renderWithClient(HistoryPane, {
			...base,
			hasMore: true,
			loadingMore: true,
			onloadmore: vi.fn()
		});
		expect(observers.filter((o) => !o.disconnected)).toEqual([]);
	});

	it('読み込みに失敗したときは自動で繰り返さず、もう一度のボタンで読み込む', async () => {
		const onloadmore = vi.fn();
		renderWithClient(HistoryPane, {
			...base,
			hasMore: true,
			loadMoreFailed: true,
			onloadmore
		});
		expect(observers.filter((o) => !o.disconnected)).toEqual([]);
		await fireEvent.click(screen.getByRole('button', { name: t('history.load_more_retry') }));
		expect(onloadmore).toHaveBeenCalledTimes(1);
	});

	it('保存が 1 件だけなら、保存するたびに並ぶ旨の案内を維持する', () => {
		renderWithClient(HistoryPane, { ...base, entries: buildTimeline(points.slice(0, 1)) });
		expect(screen.getByText(t('timeline.empty_message'))).toBeTruthy();
	});
});
