import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import type { ConflictFile } from '#lib/api/types.js';
import { alternateName, conflictDetail, conflictOptions } from './conflict-item';

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

describe('日時が不明な場合の別名', () => {
	it('日付を付けず、版の名前だけを括弧に入れる', () => {
		const name = alternateName('docs/report.docx', 'cloud', null);
		expect(name).toMatch(/^docs\/report \([^)\s]+\)\.docx$/);
		expect(name).not.toMatch(/\d\d-\d\d/);
	});
});

describe('各版の説明文', () => {
	const base: ConflictFile = {
		path: 'a.txt',
		kind: 'both',
		thisPcSavedAt: new Date(2026, 9, 6, 18, 2),
		cloudSavedAt: new Date(2026, 9, 6, 9, 30),
		thisPcName: null,
		cloudPcName: null
	};

	it('PC 名が無ければ従来の文言（日時だけ）', () => {
		expect(conflictDetail('this', base)).toBe(t('conflict.this_pc_detail', { time: '10/6 18:02' }));
		expect(conflictDetail('cloud', base)).toBe(
			t('conflict.cloud_detail_no_pc', { time: '10/6 09:30' })
		);
	});

	it('PC 名があれば説明に含める', () => {
		const named = { ...base, thisPcName: 'デスクトップPC', cloudPcName: 'ノートPC' };
		expect(conflictDetail('this', named)).toContain('デスクトップPC');
		expect(conflictDetail('cloud', named)).toContain('ノートPC');
	});

	it('日時が無くても PC 名だけで説明できる。両方無ければ説明を出さない', () => {
		const onlyName = { ...base, thisPcSavedAt: null, cloudSavedAt: null, cloudPcName: 'ノートPC' };
		expect(conflictDetail('cloud', onlyName)).toBe(t('conflict.cloud_only_pc', { pc: 'ノートPC' }));
		expect(conflictDetail('this', onlyName)).toBeUndefined();
	});
});
