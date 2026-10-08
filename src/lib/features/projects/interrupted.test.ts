import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { interruptedOperationName } from './interrupted';

describe('途中で止まった操作の名前', () => {
	it('操作名を画面用の言葉にする', () => {
		expect(interruptedOperationName('save')).toBe(t('interrupted.op_save'));
		expect(interruptedOperationName('pull')).toBe(t('interrupted.op_pull'));
		expect(interruptedOperationName('push')).toBe(t('interrupted.op_push'));
		expect(interruptedOperationName('restore')).toBe(t('interrupted.op_restore'));
		expect(interruptedOperationName('resolve')).toBe(t('interrupted.op_resolve'));
	});

	it('知らない操作名や空でも汎用の言葉にする（内部の名前を画面に出さない）', () => {
		expect(interruptedOperationName('merge')).toBe(t('interrupted.op_other'));
		expect(interruptedOperationName(null)).toBe(t('interrupted.op_other'));
	});
});
