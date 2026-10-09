import { describe, it, expect } from 'vitest';
import { blobUrl, revForPoint } from './image-source';

describe('revForPoint', () => {
	it('「いま」は作業フォルダ work、それ以外の時点は ID をそのまま使う', () => {
		expect(revForPoint('current')).toBe('work');
		expect(revForPoint('a1b2c3d')).toBe('a1b2c3d');
	});
});

describe('blobUrl', () => {
	it('macOS・Linux の形は app-blob://localhost/<プロジェクト>/<rev>/<エンコードしたパス>', () => {
		expect(blobUrl('proj-1', 'work', '資料/図 1.png', false)).toBe(
			`app-blob://localhost/proj-1/work/${encodeURIComponent('資料/図 1.png')}`
		);
	});

	it('Windows の形は http://app-blob.localhost/ になる', () => {
		expect(blobUrl('proj-1', 'abc', 'a.png', true)).toBe(
			'http://app-blob.localhost/proj-1/abc/a.png'
		);
	});

	it('パスの中の `/` と `?` `#` はエンコードされ、区切りとして解釈されない', () => {
		const url = blobUrl('p', 'work', 'dir/a?b#c.png', false);
		// 'app-blob:' '' 'localhost' プロジェクト rev パス（パス内の / は含まない）
		expect(url.split('/')).toHaveLength(6);
		expect(url.endsWith(encodeURIComponent('dir/a?b#c.png'))).toBe(true);
	});
});
