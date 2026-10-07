import { describe, it, expect } from 'vitest';
import { alternateName, conflictOptions } from './conflict-item';

describe('ぶつかり項目', () => {
	it('両方変更型は「この PC の版」と「クラウドの版」の2択', () => {
		expect(conflictOptions('both')).toEqual([
			{ choice: 'this', label: 'conflict.use_this' },
			{ choice: 'cloud', label: 'conflict.use_cloud' }
		]);
	});

	it('クラウドで削除された型は、この PC の版が「残す」、クラウドの版が「削除を受け入れる」', () => {
		const options = conflictOptions('deleted_in_cloud');
		expect(options.find((o) => o.label === 'conflict.keep_file')?.choice).toBe('this');
		expect(options.find((o) => o.label === 'conflict.accept_delete')?.choice).toBe('cloud');
	});

	it('この PC で削除された型は対応が逆になる', () => {
		const options = conflictOptions('deleted_on_this_pc');
		expect(options.find((o) => o.label === 'conflict.keep_file')?.choice).toBe('cloud');
		expect(options.find((o) => o.label === 'conflict.accept_delete')?.choice).toBe('this');
	});

	it('別名は「名前 (版 月-日).拡張子」の形になる', () => {
		const name = alternateName('docs/report.docx', 'cloud', new Date(2026, 9, 6));
		expect(name.startsWith('docs/report (')).toBe(true);
		expect(name.endsWith(' 10-06).docx')).toBe(true);
	});

	it('拡張子が無いファイルにも使える', () => {
		const name = alternateName('README', 'this', new Date(2026, 0, 2));
		expect(name.startsWith('README (')).toBe(true);
		expect(name.endsWith(' 01-02)')).toBe(true);
	});
});
