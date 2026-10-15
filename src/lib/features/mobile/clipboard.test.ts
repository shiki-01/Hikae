import { describe, expect, it } from 'vitest';
import { imageTypeOf } from './clipboard';

describe('クリップボードの型の選び方', () => {
	it('画像の型があれば、png を優先して選ぶ', () => {
		expect(imageTypeOf(['text/plain', 'image/jpeg', 'image/png'])).toBe('image/png');
		expect(imageTypeOf(['image/gif'])).toBe('image/gif');
	});

	it('画像の型が無ければ null（文字列だけのとき）', () => {
		expect(imageTypeOf(['text/plain', 'text/html'])).toBeNull();
	});
});
