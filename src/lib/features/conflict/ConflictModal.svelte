<script lang="ts">
	import { createQueries } from '@tanstack/svelte-query';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import { api } from '#lib/api/index.js';
	import { keys } from '#lib/api/keys.js';
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
	import LineEditor from './LineEditor.svelte';
	import {
		composeResolved,
		initialPicks,
		parseConflictMarkers,
		setPick,
		type ConflictDocument,
		type HunkPick
	} from './conflict-markers';
	import {
		canResolve,
		setChoice,
		setEdit,
		setKeepBoth,
		syncSelection,
		toEdits,
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

	/** 行ごとに編集するための編集中の状態。ファイル 1 件分（開いている間は全文を固定する） */
	interface LineEditing {
		path: string;
		doc: ConflictDocument;
		picks: (HunkPick | null)[];
	}

	let selection = $state<Selection>({});
	let comparing = $state<ConflictFile | null>(null);
	let lineEditing = $state<LineEditing | null>(null);

	const list = $derived(conflicts.data ?? []);
	const busy = $derived(resolve.isPending || abort.isPending);
	const remaining = $derived(unresolvedCount(list, selection));
	const ready = $derived(canResolve(list, selection));

	/** 行ごとに選べるか調べる対象（両方変更・両方追加・両方削除。全文が無いものは null が返る） */
	const lineCandidates = $derived(list.filter((c) => c.kind === 'both'));
	const lineQueries = createQueries(() => ({
		queries: lineCandidates.map((c) => ({
			queryKey: keys.conflictText(projectId, c.path),
			queryFn: async () => ({ path: c.path, text: await api.conflictText(projectId, c.path) }),
			enabled: open && projectId !== '',
			refetchOnWindowFocus: false
		}))
	}));

	/** 行ごとに選べるファイルの解析結果（パス → 解析できた全文）。解析できないものは含めない */
	const lineDocs = $derived.by(() => {
		const docs: Record<string, ConflictDocument> = {};
		for (const query of lineQueries) {
			const data = query.data;
			if (!data || data.text === null) continue;
			const doc = parseConflictMarkers(data.text);
			if (doc) docs[data.path] = doc;
		}
		return docs;
	});

	$effect(() => {
		if (conflicts.data) selection = syncSelection(conflicts.data, selection);
	});

	$effect(() => {
		if (!open) {
			selection = {};
			comparing = null;
			lineEditing = null;
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

	/** 行ごとの編集を開く。このファイルの選択（2 択・編集済み）は取り消し、選び直しにする */
	function openLineEditor(path: string) {
		const doc = lineDocs[path];
		if (!doc) return;
		selection = setEdit(selection, path, null);
		lineEditing = { path, doc, picks: initialPicks(doc) };
	}

	function closeLineEditor() {
		lineEditing = null;
	}

	/** ハンクの選択を変える。全ハンクが決まると、そのファイルは行ごとの編集で確定する */
	function pickHunk(index: number, pick: HunkPick) {
		if (!lineEditing) return;
		const picks = setPick(lineEditing.picks, index, pick);
		lineEditing = { ...lineEditing, picks };
		selection = setEdit(selection, lineEditing.path, composeResolved(lineEditing.doc, picks));
	}

	function later() {
		if (!busy) abort.mutate();
	}

	function submit() {
		if (ready) {
			resolve.mutate({
				resolutions: toResolutions(list, selection),
				edits: toEdits(list, selection)
			});
		}
	}
</script>

{#snippet item(conflict: ConflictFile)}
	<ConflictItem
		{conflict}
		choice={selection[conflict.path]?.choice ?? null}
		keepBoth={selection[conflict.path]?.keepBoth ?? false}
		edited={selection[conflict.path]?.edit != null}
		lineEditable={lineDocs[conflict.path] !== undefined}
		lineOpen={lineEditing?.path === conflict.path}
		onlineedit={() =>
			lineEditing?.path === conflict.path ? closeLineEditor() : openLineEditor(conflict.path)}
		onchoice={(choice: Choice) => (selection = setChoice(selection, conflict.path, choice))}
		onkeepboth={(keep) => (selection = setKeepBoth(selection, conflict.path, keep))}
		oncompare={() => (comparing = conflict)}
	/>
{/snippet}
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

	<div
		class={`${lineEditing ? 'max-w:1200px' : 'max-w:720px'} mx:auto flex flex-direction:column gap:4`}
		inert={comparing !== null}
	>
		{#if conflicts.isPending}
			<Skeleton class="h:160px" />
			<Skeleton class="h:160px" />
		{:else}
			{#each list as conflict (conflict.path)}
				{#if lineEditing && lineEditing.path === conflict.path}
					<div
						class="grid grid-template-columns:minmax(0,1fr)|minmax(0,2fr) gap:4 align-items:start"
					>
						{@render item(conflict)}
						<LineEditor
							doc={lineEditing.doc}
							picks={lineEditing.picks}
							onpick={pickHunk}
							onclose={closeLineEditor}
						/>
					</div>
				{:else}
					{@render item(conflict)}
				{/if}
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
