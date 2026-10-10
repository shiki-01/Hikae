import { describe, it, expect } from 'vitest';
import { defaultDetailView } from './detail-view';

describe('defaultDetailView', () => {
	it('「変更のみ」では差分、「すべてのファイル」ではファイルの内容を既定にする', () => {
		expect(defaultDetailView('changes')).toBe('diff');
		expect(defaultDetailView('tree')).toBe('preview');
	});
});
