// テスト用の `$app/navigation`。画面遷移は実行せず、呼ばれた先だけを記録する
import { vi } from 'vitest';

export const goto = vi.fn(async (_url: string | URL) => {});
export const invalidate = vi.fn(async () => {});
export const invalidateAll = vi.fn(async () => {});
