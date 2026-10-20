import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import {
	interruptedDialogStep,
	interruptedOperationName,
	isResumableOperation
} from './interrupted';

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

	it('待機中の「続きを行う」は復旧と続きの操作を始める（続きの実行は呼び出し側）', () => {
		expect(interruptedDialogStep('idle', 'resume')).toEqual({
			phase: 'resuming',
			run: 'resume',
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
		for (const phase of ['recovering', 'resuming', 'dismissing'] as const) {
			for (const action of ['recover', 'resume', 'dismiss', 'close'] as const) {
				expect(interruptedDialogStep(phase, action)).toEqual({
					phase,
					run: null,
					close: false
				});
			}
		}
	});
});

describe('続きを行える操作', () => {
	it('取り込みとアップロードだけ続きを行える', () => {
		expect(isResumableOperation('pull')).toBe(true);
		expect(isResumableOperation('push')).toBe(true);
	});

	it('保存・元に戻す・ぶつかり解消・知らない操作・空は続きを行えない', () => {
		expect(isResumableOperation('save')).toBe(false);
		expect(isResumableOperation('restore')).toBe(false);
		expect(isResumableOperation('resolve')).toBe(false);
		expect(isResumableOperation('merge')).toBe(false);
		expect(isResumableOperation(null)).toBe(false);
	});
});
