import { describe, it, expect } from 'vitest';
import { NUMBER_RANGES, parseCustomNumber } from './number-input';

const range = { min: 10, max: 3600 };

describe('カスタム数値の検証', () => {
	it('範囲内の整数を受け付ける（端を含む）', () => {
		expect(parseCustomNumber('45', range)).toEqual({ ok: true, value: 45 });
		expect(parseCustomNumber('10', range)).toEqual({ ok: true, value: 10 });
		expect(parseCustomNumber('3600', range)).toEqual({ ok: true, value: 3600 });
	});

	it('範囲外は受け付けない', () => {
		expect(parseCustomNumber('9', range).ok).toBe(false);
		expect(parseCustomNumber('3601', range).ok).toBe(false);
		expect(parseCustomNumber('0', range).ok).toBe(false);
	});

	it('空・小数・符号・文字は受け付けない', () => {
		for (const raw of ['', '   ', '1.5', '-20', '+20', '20秒', 'abc', '1e3', '2 0', '0x20']) {
			expect(parseCustomNumber(raw, range).ok, raw).toBe(false);
		}
	});

	it('全角の数字と前後の空白は読み取る', () => {
		expect(parseCustomNumber('４５', range)).toEqual({ ok: true, value: 45 });
		expect(parseCustomNumber('  120 ', range)).toEqual({ ok: true, value: 120 });
		expect(parseCustomNumber('０１２０', range)).toEqual({ ok: true, value: 120 });
	});

	it('極端に長い数字は受け付けない', () => {
		expect(parseCustomNumber('9'.repeat(40), range).ok).toBe(false);
	});
});

describe('範囲の定義', () => {
	it('設計書 7章の範囲と一致する（バックエンドの検証と同じ値）', () => {
		expect(NUMBER_RANGES).toEqual({
			pullIntervalMinutes: { min: 1, max: 1440 },
			autoSnapshotDelaySecs: { min: 10, max: 3600 },
			snapshotRetentionDays: { min: 7, max: 730 },
			largeFileWarnMb: { min: 1, max: 100 }
		});
	});
});
