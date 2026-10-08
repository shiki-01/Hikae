import { describe, it, expect } from 'vitest';
import { AppError } from '#lib/api/errors.js';
import { t } from '#lib/i18n/index.js';
import {
	failureForOutcome,
	formatCountdown,
	isHttpsUrl,
	isLoginSetupError,
	setupFailure
} from './login-flow';

describe('開いてよいリンクの判定', () => {
	it('https の URL だけを許す', () => {
		expect(isHttpsUrl('https://github.com/login/device')).toBe(true);
		expect(isHttpsUrl('http://github.com/login/device')).toBe(false);
		expect(isHttpsUrl('not a url')).toBe(false);
	});
});

const backendMessage = {
	code: 'login_not_configured',
	params: {},
	whatHappened: 'a',
	dataIsSafe: 'b',
	nextAction: 'c'
};

describe('ログインの設定が無いエラーの判定', () => {
	it('クライアント ID の未設定と Device Flow の無効を設定が必要なエラーとして扱う', () => {
		for (const technical of ['MissingClientId', 'DeviceFlowDisabled']) {
			expect(isLoginSetupError(new AppError('backend', technical, {}, backendMessage))).toBe(true);
		}
	});

	it('通信の失敗など、ほかのエラーは設定の問題として扱わない', () => {
		expect(
			isLoginSetupError(new AppError('backend', 'NetworkUnavailable', {}, backendMessage))
		).toBe(false);
		expect(isLoginSetupError(new Error('MissingClientId'))).toBe(false);
		expect(isLoginSetupError(null)).toBe(false);
	});
});

describe('ログインの結末の表示', () => {
	it('期限切れと拒否は 3 要素で、もう一度ログインできる', () => {
		for (const outcome of ['expired', 'denied'] as const) {
			const view = failureForOutcome(outcome);
			expect(view?.retryable).toBe(true);
			expect(t(view!.title)).not.toBe('');
			expect(t(view!.dataIsSafe)).toContain('安全');
			expect(t(view!.nextAction)).not.toBe('');
		}
	});

	it('成功とキャンセルは失敗として扱わない', () => {
		expect(failureForOutcome('succeeded')).toBeNull();
		expect(failureForOutcome('canceled')).toBeNull();
	});

	it('設定が必要な場合は、何度試しても同じなので再試行を出さない', () => {
		expect(setupFailure().retryable).toBe(false);
	});
});

describe('有効期限の残り時間', () => {
	it('分と秒で表す', () => {
		expect(formatCountdown(900)).toBe('15:00');
		expect(formatCountdown(61)).toBe('1:01');
		expect(formatCountdown(9.9)).toBe('0:09');
	});

	it('0 を下回らない', () => {
		expect(formatCountdown(-5)).toBe('0:00');
	});
});
