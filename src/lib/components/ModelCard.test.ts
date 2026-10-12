// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { AiModel } from '#lib/api/types.js';
import { setupSmoke } from '../../test/smoke';
import ModelCard from './ModelCard.svelte';

setupSmoke();

function model(overrides: Partial<AiModel> = {}): AiModel {
	return {
		id: 'm',
		displayName: 'テスト用モデル',
		stage: 2,
		sizeBytes: 3 * 1024 ** 3,
		licenseName: 'Apache License 2.0',
		licenseUrl: 'https://www.apache.org/licenses/LICENSE-2.0',
		minMemoryBytes: 7 * 1024 ** 3,
		recommended: false,
		recommendedForThisPc: false,
		status: 'missing',
		partialBytes: 0,
		available: true,
		unavailableReason: null,
		downloading: false,
		...overrides
	};
}

function renderCard(m: AiModel) {
	return render(ModelCard, { props: { model: m, selected: false, onaction: () => {} } });
}

describe('モデルの段階の表示（軽量・標準）', () => {
	afterEach(cleanup);

	it('段階 1 は「段階 1（軽量）」と、内容の理解が限定的であることの説明を出す', () => {
		renderCard(model({ stage: 1 }));
		expect(screen.getByText(/段階 1（軽量）/)).toBeTruthy();
		expect(screen.getByText(/内容の理解は限定的です/)).toBeTruthy();
		expect(screen.getByText(/「標準」をおすすめします/)).toBeTruthy();
	});

	it('段階 2 は「段階 2（標準）」と出し、軽量の説明は出さない', () => {
		renderCard(model({ stage: 2 }));
		expect(screen.getByText(/段階 2（標準）/)).toBeTruthy();
		expect(screen.queryByText(/内容の理解は限定的です/)).toBeNull();
	});
});

describe('推奨のバッジ', () => {
	afterEach(cleanup);

	it('この PC のおすすめのモデルには「この PC のおすすめ」を出す', () => {
		renderCard(model({ recommendedForThisPc: true }));
		expect(screen.getByText('この PC のおすすめ')).toBeTruthy();
		expect(screen.queryByText('推奨')).toBeNull();
	});

	it('この PC のおすすめでない段階の推奨（既定）は「推奨」を出す', () => {
		renderCard(model({ recommended: true, recommendedForThisPc: false }));
		expect(screen.getByText('推奨')).toBeTruthy();
		expect(screen.queryByText('この PC のおすすめ')).toBeNull();
	});

	it('どちらでもないモデルは、バッジを出さない', () => {
		renderCard(model({ recommended: false, recommendedForThisPc: false }));
		expect(screen.queryByText('この PC のおすすめ')).toBeNull();
		expect(screen.queryByText('推奨')).toBeNull();
	});
});
