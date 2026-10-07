import { describe, it, expect } from 'vitest';
import { t } from './index';

describe('i18n', () => {
	it('定義済みキーの日本語文言を返す', () => {
		expect(t('app.name')).toBe('Hikae');
	});
});
