<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import type { Choice, ConflictFile } from '#lib/api/types.js';
	import Badge from '#lib/components/Badge.svelte';
	import Button from '#lib/components/Button.svelte';
	import ConflictItem from '#lib/components/ConflictItem.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import type { SelectOption } from '#lib/components/Select.svelte';
	import CompareView from '#lib/features/compare/CompareView.svelte';
	import { useAbortMerge, useResolve } from './mutations';
	import { useConflicts } from './queries';
	import {
		canResolve,
		setChoice,
		setKeepBoth,
		syncSelection,
		toResolutions,
		unresolvedCount,
		type Selection
	} from './selection';

	interface Props {
		open: boolean;
		projectId: string;
		onclose: () => void;
	}

	let { open, projectId, onclose }: Props = $props();

	const conflicts = useConflicts(
		() => projectId,
		() => open
	);
	const resolve = useResolve(
		() => projectId,
		() => onclose()
	);
	const abort = useAbortMerge(
		() => projectId,
		() => onclose()
	);

	let selection = $state<Selection>({});
	let comparing = $state<ConflictFile | null>(null);

	const list = $derived(conflicts.data ?? []);
	const busy = $derived(resolve.isPending || abort.isPending);
	const remaining = $derived(unresolvedCount(list, selection));
	const ready = $derived(canResolve(list, selection));

	$effect(() => {
		if (conflicts.data) selection = syncSelection(conflicts.data, selection);
	});

	$effect(() => {
		if (!open) {
			selection = {};
			comparing = null;
		}
	});

	function pointOptions(conflict: ConflictFile): SelectOption[] {
		return [
			{
				value: 'this',
				label: t('conflict.this_pc'),
				description: conflict.thisPcSavedAt
					? t('conflict.saved_at', { time: formatDateTime(conflict.thisPcSavedAt) })
					: undefined
			},
			{
				value: 'cloud',
				label: t('conflict.cloud'),
				description: conflict.cloudSavedAt
					? t('conflict.saved_at', { time: formatDateTime(conflict.cloudSavedAt) })
					: undefined
			}
		];
	}

	function later() {
		if (!busy) abort.mutate();
	}

	function submit() {
		if (ready) resolve.mutate(toResolutions(list, selection));
	}
</script>

<Dialog
	{open}
	variant="fullscreen"
	title={t('conflict.title')}
	description={t('conflict.description')}
	hideClose
	{busy}
	onclose={later}
>
	{#snippet headerExtra()}
		<Badge variant="danger" label={t('conflict.count', { count: list.length })} />
	{/snippet}

	<div class="max-w:720px mx:auto flex flex-direction:column gap:4" inert={comparing !== null}>
		{#if conflicts.isPending}
			<Skeleton class="h:160px" />
			<Skeleton class="h:160px" />
		{:else}
			{#each list as conflict (conflict.path)}
				<ConflictItem
					{conflict}
					choice={selection[conflict.path]?.choice ?? null}
					keepBoth={selection[conflict.path]?.keepBoth ?? false}
					onchoice={(choice: Choice) => (selection = setChoice(selection, conflict.path, choice))}
					onkeepboth={(keep) => (selection = setKeepBoth(selection, conflict.path, keep))}
					oncompare={() => (comparing = conflict)}
				/>
			{/each}
		{/if}
	</div>

	{#if comparing}
		<CompareView
			{projectId}
			path={comparing.path}
			points={pointOptions(comparing)}
			initialFrom="this"
			initialTo="cloud"
			onclose={() => (comparing = null)}
		/>
	{/if}

	{#snippet actions()}
		<p class="m:0 flex:1 type-body fg:fg-muted" aria-live="polite">
			{remaining > 0 ? t('conflict.remaining', { count: remaining }) : t('conflict.all_chosen')}
		</p>
		<Button variant="secondary" disabled={busy} loading={abort.isPending} onclick={later}>
			{t('conflict.later')}
		</Button>
		<Button disabled={!ready || busy} loading={resolve.isPending} onclick={submit}>
			{t('conflict.resolve')}
		</Button>
	{/snippet}
</Dialog>
