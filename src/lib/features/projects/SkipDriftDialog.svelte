<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { summarizeTracked } from '#lib/features/settings/ignore-model.js';
	import { useDismissSkipDrift, useReapplySkipWorktree } from '#lib/features/settings/mutations.js';
	import { useSkipDrift } from '#lib/features/settings/queries.js';

	interface Props {
		/** 開いているプロジェクト */
		projectId: string;
	}

	let { projectId }: Props = $props();

	/** プロジェクトを開いている間に、答えた（閉じた）あとは出さない（同じ提案は繰り返さない） */
	let answered = $state(false);

	const drift = useSkipDrift(() => projectId);

	const paths = $derived(!answered && drift.data && drift.data.length > 0 ? drift.data : null);
	const summary = $derived(paths ? summarizeTracked(paths) : null);

	const reapply = useReapplySkipWorktree(
		() => projectId,
		() => (answered = true)
	);
	const later = useDismissSkipDrift(
		() => projectId,
		() => (answered = true)
	);
</script>

<Dialog
	open={paths !== null}
	variant="confirm"
	title={t('skip.drift_title')}
	description={t('skip.drift_description')}
	busy={reapply.isPending || later.isPending}
	onclose={() => (answered = true)}
>
	{#if summary}
		<div class="flex flex-direction:column gap:2">
			<ul
				class="m:0 p:0 list-style:none b:1px|solid|border r:md"
				aria-label={t('skip.drift_list_label')}
			>
				{#each summary.shown as path (path)}
					<li class="type-body px:3 py:2 bb:1px|solid|border overflow-wrap:anywhere">{path}</li>
				{/each}
			</ul>
			{#if summary.rest > 0}
				<p class="m:0 type-small fg:fg-muted">
					{t('ignore.untrack_more', { count: summary.rest })}
				</p>
			{/if}
		</div>
	{/if}

	{#snippet footerNote()}
		<p class="m:0 type-body">{t('skip.drift_safe')}</p>
	{/snippet}

	{#snippet actions()}
		<div class="flex flex-flow:row|wrap justify-content:end gap:3 flex:1">
			<Button
				variant="ghost"
				disabled={reapply.isPending || later.isPending}
				onclick={() => {
					if (paths) later.mutate(paths);
				}}
			>
				{t('skip.drift_later')}
			</Button>
			<Button
				variant="primary"
				loading={reapply.isPending}
				disabled={reapply.isPending || later.isPending}
				onclick={() => {
					if (paths) reapply.mutate(paths);
				}}
			>
				{t('skip.drift_apply')}
			</Button>
		</div>
	{/snippet}
</Dialog>
