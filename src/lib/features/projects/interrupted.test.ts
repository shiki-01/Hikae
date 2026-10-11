import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { interruptedDialogStep, interruptedOperationName } from './interrupted';

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

describe('中断ダイアログの状態遷移', () => {
	it('待機中の「操作前の状態に戻す」は復旧を始め、閉じるまでは出さない', () => {
		expect(interruptedDialogStep('idle', 'recover')).toEqual({
			phase: 'recovering',
			run: 'recover',
			close: false
		});
	});

	it('待機中の「このまま続ける」は中断の印を閉じる処理を始める', () => {
		expect(interruptedDialogStep('idle', 'dismiss')).toEqual({
			phase: 'dismissing',
			run: 'dismiss',
			close: false
		});
	});

	it('待機中の「閉じる」は処理を始めず、ダイアログだけを閉じる', () => {
		expect(interruptedDialogStep('idle', 'close')).toEqual({
			phase: 'idle',
			run: null,
			close: true
		});
	});

	it('処理中は、どの操作も受け付けない（閉じるも含む）', () => {
		for (const phase of ['recovering', 'dismissing'] as const) {
			for (const action of ['recover', 'dismiss', 'close'] as const) {
				expect(interruptedDialogStep(phase, action)).toEqual({
					phase,
					run: null,
					close: false
				});
			}
		}
	});
});
