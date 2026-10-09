import { createMutation, useQueryClient } from '@tanstack/svelte-query';
import { api } from '#lib/api/index.js';
import { keys } from '#lib/api/keys.js';
import { reportError } from '#lib/features/notifications/store.svelte.js';

/**
 * ログアウト。この PC に保存したログイン情報だけを消す（ファイルとローカルの履歴は消えない）。
 * 成功したら、ログイン状態に依存する取得結果を捨ててから `onDone` を呼ぶ。
 */
export function useLogout(onDone?: () => void) {
	const client = useQueryClient();
	return createMutation(() => ({
		mutationFn: () => api.logout(),
		onSuccess: async () => {
			client.removeQueries({ queryKey: keys.owners });
			client.removeQueries({ queryKey: ['remote-projects'] });
			await client.invalidateQueries({ queryKey: keys.session });
			onDone?.();
		},
		onError: (error) => reportError(error)
	}));
}
