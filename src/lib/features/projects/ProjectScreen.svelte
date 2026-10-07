<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Upload } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import { api } from '#lib/api/index.js';
	import type { OpenTarget, RestoreScope, SavePoint } from '#lib/api/types.js';
	import SaveBar from '#lib/components/SaveBar.svelte';
	import type { SelectOption } from '#lib/components/Select.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import StatusHeader from '#lib/components/StatusHeader.svelte';
	import { buildTimeline, latestManualPoint } from '#lib/components/timeline.js';
	import ChangeDetail from '#lib/features/changes/ChangeDetail.svelte';
	import ChangesPane from '#lib/features/changes/ChangesPane.svelte';
	import { classifyDropped } from '#lib/features/changes/files.js';
	import { suggestMemo } from '#lib/features/changes/memo.js';
	import { useAddFiles, useSave } from '#lib/features/changes/mutations.js';
	import { useChanges } from '#lib/features/changes/queries.js';
	import CompareView from '#lib/features/compare/CompareView.svelte';
	import { pointOptions } from '#lib/features/compare/navigation.js';
	import ConflictModal from '#lib/features/conflict/ConflictModal.svelte';
	import HistoryDetail from '#lib/features/history/HistoryDetail.svelte';
	import HistoryPane from '#lib/features/history/HistoryPane.svelte';
	import RestoreDialog from '#lib/features/history/RestoreDialog.svelte';
	import { useRestore } from '#lib/features/history/mutations.js';
	import { useHistory } from '#lib/features/history/queries.js';
	import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
	import { network } from '#lib/utils/online.svelte.js';
	import { useFetch, usePush } from './sync';
	import { useProject } from './queries';

	interface Props {
		projectId: string;
	}

	let { projectId }: Props = $props();

	const PANE_MIN = 280;
	const PANE_MAX = 360;

	const project = useProject(() => projectId);
	const changesQuery = useChanges(() => projectId);
	const historyQuery = useHistory(() => projectId);

	let tab = $state<'changes' | 'history'>('changes');
	let selectedPath = $state<string | null>(null);
	let selectedPointId = $state<string | null>('now');
	let expanded = $state<string[]>([]);
	let memo = $state('');
	let memoTouched = $state(false);
	let leftWidth = $state(320);
	let body = $state<HTMLDivElement>();
	let resizing = $state(false);
	let compare = $state<{ path: string; from: string; to: string; points: SelectOption[] } | null>(
		null
	);
	let restoreRequest = $state<{ point: SavePoint; scope: RestoreScope } | null>(null);
	let conflictOpen = $state(false);
	let dragDepth = $state(0);
	let tooLarge = $state(false);

	const changes = $derived(changesQuery.data ?? []);
	const history = $derived(historyQuery.data ?? []);
	const latest = $derived(latestManualPoint(history));
	const entries = $derived(buildTimeline(history));
	const selectedChange = $derived(changes.find((c) => c.path === selectedPath) ?? null);
	const selectedPoint = $derived(history.find((p) => p.id === selectedPointId) ?? null);
	const suggestion = $derived(suggestMemo(changes));
	const baseId = $derived(latest?.id ?? 'current');

	const save = useSave(
		() => projectId,
		() => {
			memo = '';
			memoTouched = false;
		}
	);
	const fetchMutation = useFetch(
		() => projectId,
		() => (conflictOpen = true)
	);
	const push = usePush(() => projectId);
	const addFiles = useAddFiles(() => projectId);
	const restore = useRestore(
		() => projectId,
		() => (restoreRequest = null)
	);

	const syncing = $derived(fetchMutation.isPending ? 'fetch' : push.isPending ? 'push' : null);

	$effect(() => {
		if (!memoTouched) memo = suggestion;
	});

	$effect(() => {
		if (changes.length === 0) {
			selectedPath = null;
		} else if (!changes.some((c) => c.path === selectedPath)) {
			selectedPath = changes[0].path;
		}
	});

	function submitSave() {
		const text = memo.trim() || suggestion;
		if (text) save.mutate(text);
	}

	function openCompare(path: string, from: string, to: string) {
		compare = { path, from, to, points: pointOptions(history) };
	}

	function askRestore(point: SavePoint | undefined, scope: RestoreScope) {
		if (point) restoreRequest = { point, scope };
	}

	async function openFile(path: string, target: OpenTarget) {
		try {
			if (target === 'copy_path') {
				await navigator.clipboard.writeText(`${project.data?.path ?? ''}\\${path}`);
				pushToast({ type: 'info', message: t('toast.path_copied') });
				return;
			}
			await api.openFile(projectId, path, target);
		} catch (error) {
			reportError(error);
		}
	}

	function onFiles(files: File[]) {
		const { accepted, rejected } = classifyDropped(
			files.map((f) => ({ name: f.name, size: f.size }))
		);
		if (rejected.length > 0) {
			tooLarge = true;
			setTimeout(() => (tooLarge = false), 4000);
		}
		if (accepted.length > 0) {
			tab = 'changes';
			addFiles.mutate(accepted);
		}
	}

	function hasFiles(event: DragEvent): boolean {
		return event.dataTransfer?.types.includes('Files') ?? false;
	}

	function toggleExpanded(id: string) {
		expanded = expanded.includes(id) ? expanded.filter((e) => e !== id) : [...expanded, id];
	}

	function clampWidth(value: number): number {
		return Math.min(PANE_MAX, Math.max(PANE_MIN, value));
	}

	function startResize(event: PointerEvent) {
		resizing = true;
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function moveResize(event: PointerEvent) {
		if (!resizing || !body) return;
		leftWidth = clampWidth(event.clientX - body.getBoundingClientRect().left);
	}

	function resizeKey(event: KeyboardEvent) {
		if (event.key === 'ArrowLeft') leftWidth = clampWidth(leftWidth - 16);
		else if (event.key === 'ArrowRight') leftWidth = clampWidth(leftWidth + 16);
		else return;
		event.preventDefault();
	}

	const baseLabel = $derived(latest ? formatDateTime(latest.createdAt) : '');
</script>

<svelte:window
	ondragenter={(event) => {
		if (hasFiles(event)) dragDepth += 1;
	}}
	ondragover={(event) => {
		if (hasFiles(event)) event.preventDefault();
	}}
	ondragleave={(event) => {
		if (hasFiles(event)) dragDepth = Math.max(0, dragDepth - 1);
	}}
	ondrop={(event) => {
		if (!hasFiles(event)) return;
		event.preventDefault();
		dragDepth = 0;
		if (event.dataTransfer?.files.length) onFiles(Array.from(event.dataTransfer.files));
	}}
/>

<div class="h:100vh flex flex-direction:column bg:bg fg:fg min-w:960px">
	<StatusHeader
		projectName={project.data?.name ?? ''}
		hasConflict={project.data?.hasConflict ?? false}
		{syncing}
		isOnline={network.online}
		unsavedCount={changesQuery.data ? changes.length : (project.data?.unsavedCount ?? 0)}
		uploadPendingCount={project.data?.uploadPendingCount ?? 0}
		fetchPendingCount={project.data?.fetchPendingCount ?? 0}
		lastUploadedAt={project.data?.lastUploadedAt ?? null}
		onback={() => goto(resolve('/'))}
		onsettings={() => goto(resolve('/settings'))}
		onfetch={() => fetchMutation.mutate()}
		onpush={() => push.mutate()}
		onretry={() => fetchMutation.mutate()}
		onreview={() => (conflictOpen = true)}
	/>

	<div bind:this={body} class="position:relative flex:1 min-h:0 flex">
		<div
			class="flex flex-direction:column flex-shrink:0 bg:bg-subtle"
			style:width={`${leftWidth}px`}
			inert={compare !== null}
		>
			<div class="p:3 bb:1px|solid|border flex-shrink:0">
				<SegmentedControl
					bind:value={tab}
					ariaLabel={t('tabs.label')}
					options={[
						{ value: 'changes', label: t('tabs.changes') },
						{ value: 'history', label: t('tabs.history') }
					]}
				/>
			</div>
			{#if tab === 'changes'}
				<ChangesPane
					{changes}
					loading={changesQuery.isPending}
					{selectedPath}
					lastSavedAt={project.data?.lastSavedAt ?? null}
					dragging={dragDepth > 0}
					{tooLarge}
					onselect={(path) => (selectedPath = path)}
					onfiles={(files) => {
						dragDepth = 0;
						onFiles(files);
					}}
				/>
			{:else}
				<HistoryPane
					{entries}
					loading={historyQuery.isPending}
					selectedId={selectedPointId}
					{expanded}
					onselect={(id) => (selectedPointId = id)}
					ontoggle={toggleExpanded}
				/>
			{/if}
		</div>

		<div
			role="slider"
			aria-orientation="horizontal"
			aria-label={t('layout.resize')}
			aria-valuenow={leftWidth}
			aria-valuemin={PANE_MIN}
			aria-valuemax={PANE_MAX}
			tabindex="0"
			onpointerdown={startResize}
			onpointermove={moveResize}
			onpointerup={() => (resizing = false)}
			onkeydown={resizeKey}
			class={`w:5px flex-shrink:0 cursor:col-resize ${
				resizing ? 'bg:accent' : 'bg:border bg:accent:hover'
			}`}
		></div>

		<div class="flex flex-direction:column flex:1 min-w:0 min-h:0" inert={compare !== null}>
			{#if tab === 'changes'}
				<ChangeDetail
					{projectId}
					change={selectedChange}
					{baseId}
					{baseLabel}
					oncompare={() => selectedChange && openCompare(selectedChange.path, baseId, 'current')}
					onrestore={() =>
						selectedChange && askRestore(latest, { kind: 'file', path: selectedChange.path })}
					onopen={(target) => selectedChange && openFile(selectedChange.path, target)}
				/>
			{:else}
				<HistoryDetail
					{projectId}
					point={selectedPoint}
					isNow={selectedPointId === 'now'}
					oncompare={(path) => selectedPoint && openCompare(path, selectedPoint.id, 'current')}
					onrestore={(path) =>
						askRestore(selectedPoint ?? undefined, path ? { kind: 'file', path } : { kind: 'all' })}
					onopen={openFile}
				/>
			{/if}
		</div>

		{#if compare}
			<CompareView
				{projectId}
				path={compare.path}
				points={compare.points}
				initialFrom={compare.from}
				initialTo={compare.to}
				onclose={() => (compare = null)}
				onopen={() => compare && openFile(compare.path, 'default')}
			/>
		{/if}
	</div>

	{#if tab === 'changes'}
		<SaveBar
			bind:memo
			hasChanges={changes.length > 0}
			saving={save.isPending}
			onsave={submitSave}
		/>
	{/if}
</div>

{#if dragDepth > 0}
	<div
		class="fixed inset:0 z:100 flex align-items:center justify-content:center bg:var(--color-scrim) pointer-events:none"
		aria-hidden="true"
	>
		<div
			class="flex flex-direction:column align-items:center gap:2 p:8 r:lg bg:bg-raised shadow:overlay"
		>
			<Upload size={32} class="fg:accent" />
			<p class="m:0 type-heading">{t('dropzone.dragging')}</p>
		</div>
	</div>
{/if}

<RestoreDialog
	open={restoreRequest !== null}
	{projectId}
	point={restoreRequest?.point ?? null}
	scope={restoreRequest?.scope ?? { kind: 'all' }}
	pending={restore.isPending}
	oncancel={() => (restoreRequest = null)}
	onconfirm={() =>
		restoreRequest &&
		restore.mutate({ targetId: restoreRequest.point.id, scope: restoreRequest.scope })}
/>

<ConflictModal open={conflictOpen} {projectId} onclose={() => (conflictOpen = false)} />
