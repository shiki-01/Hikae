<script lang="ts">
	import { Trash2 } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { IgnoreKind, IgnoreRule } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import IconButton from '#lib/components/IconButton.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import IgnoreAddDialog from './IgnoreAddDialog.svelte';
	import UntrackDialog from './UntrackDialog.svelte';
	import { ignoreRuleStatus } from './ignore-model';
	import { useAddIgnorePattern, useRemoveIgnorePattern, useUntrackFiles } from './mutations';
	import { useIgnoreRules, useTrackedIgnoredFiles } from './queries';

	interface Props {
		/** 開いているプロジェクト */
		projectId: string;
	}

	let { projectId }: Props = $props();

	const rules = useIgnoreRules(() => projectId);
	const tracked = useTrackedIgnoredFiles(() => projectId);

	let addOpen = $state(false);
	/** 保存対象から外す確認で扱うファイル。null のときは閉じている */
	let untrackFiles = $state<string[] | null>(null);

	const add = useAddIgnorePattern(
		() => projectId,
		(_pattern, trackedFiles) => {
			addOpen = false;
			// 追加したパターンに当たるファイルがすでに保存されていれば、外すかどうかを尋ねる
			if (trackedFiles.length > 0) untrackFiles = trackedFiles;
		}
	);
	const remove = useRemoveIgnorePattern(() => projectId);
	const untrack = useUntrackFiles(
		() => projectId,
		() => (untrackFiles = null)
	);

	/** 確認を開く。一覧に出ている追跡中のファイルを対象にする */
	function openUntrack() {
		untrackFiles = tracked.data ?? [];
	}

	function statusText(rule: IgnoreRule): string {
		const status = ignoreRuleStatus(rule);
		if (status === 'unknown') return t('ignore.count_unknown');
		if (status === 'none') return t('ignore.count_none');
		return t('ignore.count_tracked', { count: rule.trackedCount ?? 0 });
	}
</script>

<div class="flex flex-direction:column gap:4">
	<p class="m:0 type-body fg:fg-muted">{t('ignore.description')}</p>

	{#if tracked.data && tracked.data.length > 0}
		<div
			role="status"
			class="flex flex-flow:row|wrap align-items:center justify-content:space-between gap:3 p:3 r:md bg:accent-subtle"
		>
			<span class="type-body">{t('ignore.notice', { count: tracked.data.length })}</span>
			<Button size="sm" variant="secondary" onclick={openUntrack}>
				{t('ignore.untrack_open')}
			</Button>
		</div>
	{/if}

	<div class="flex justify-content:flex-end">
		<Button size="sm" onclick={() => (addOpen = true)}>{t('ignore.add')}</Button>
	</div>

	{#if rules.isPending}
		<Skeleton class="h:48px" />
	{:else if rules.data && rules.data.length > 0}
		<ul
			class="m:0 p:0 list-style:none flex flex-direction:column"
			aria-label={t('ignore.list_label')}
		>
			{#each rules.data as rule, index (`${index}:${rule.pattern}`)}
				<li
					class="flex align-items:center justify-content:space-between gap:3 py:3 bb:1px|solid|border"
				>
					<div class="flex flex-direction:column gap:1 min-w:0">
						<span class="type-body overflow-wrap:anywhere">{rule.pattern}</span>
						<span class="type-small fg:fg-muted">{statusText(rule)}</span>
					</div>
					<div class="flex align-items:center gap:2 flex-shrink:0">
						{#if ignoreRuleStatus(rule) === 'tracked'}
							<Button size="sm" variant="ghost" onclick={openUntrack}>
								{t('ignore.untrack_open')}
							</Button>
						{/if}
						<IconButton
							label={t('ignore.remove_label', { name: rule.pattern })}
							onclick={() => remove.mutate(rule.pattern)}
						>
							<Trash2 size={18} aria-hidden="true" />
						</IconButton>
					</div>
				</li>
			{/each}
		</ul>
	{:else}
		<p class="m:0 type-body fg:fg-muted">{t('ignore.empty')}</p>
	{/if}
</div>

<IgnoreAddDialog
	open={addOpen}
	busy={add.isPending}
	onsubmit={(kind: IgnoreKind, value: string) => add.mutate({ kind, value })}
	oncancel={() => (addOpen = false)}
/>

<UntrackDialog
	files={untrackFiles}
	busy={untrack.isPending}
	onsubmit={(paths: string[]) => untrack.mutate(paths)}
	oncancel={() => (untrackFiles = null)}
/>
