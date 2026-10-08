<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Upload } from '@lucide/svelte';
	import { untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import { api, isTauri } from '#lib/api/index.js';
	import type {
		DroppedFile,
		OpenTarget,
		RestoreScope,
		SavePoint,
		SizeCheck
	} from '#lib/api/types.js';
	import SaveBar from '#lib/components/SaveBar.svelte';
	import type { SelectOption } from '#lib/components/Select.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import StatusHeader from '#lib/components/StatusHeader.svelte';
	import { buildTimeline, latestManualPoint } from '#lib/components/timeline.js';
	import ChangeDetail from '#lib/features/changes/ChangeDetail.svelte';
	import ChangesPane from '#lib/features/changes/ChangesPane.svelte';
	import AddFilesResultDialog from '#lib/features/changes/AddFilesResultDialog.svelte';
	import {
		summarizeAddFiles,
		type AddFilesSummary
	} from '#lib/features/changes/add-files-result.js';
	import { nextMemo } from '#lib/features/changes/memo.js';
	import SizeCheckDialog from '#lib/features/changes/SizeCheckDialog.svelte';
	import { choiceFor, type SizeAction } from '#lib/features/changes/size-check.js';
	import { droppedFromPaths, listenNativeDrop } from '#lib/features/changes/native-drop.js';
	import { useAddFiles, useSave } from '#lib/features/changes/mutations.js';
	import { useChanges, useMemoSuggestion } from '#lib/features/changes/queries.js';
	import CompareView from '#lib/features/compare/CompareView.svelte';
	import { pointOptions } from '#lib/features/compare/navigation.js';
	import ConflictModal from '#lib/features/conflict/ConflictModal.svelte';
	import HistoryDetail from '#lib/features/history/HistoryDetail.svelte';
	import HistoryPane from '#lib/features/history/HistoryPane.svelte';
	import RestoreDialog from '#lib/features/history/RestoreDialog.svelte';
	import { useRestore } from '#lib/features/history/mutations.js';
	import { useHistory } from '#lib/features/history/queries.js';
	import { useSettings } from '#lib/features/settings/queries.js';
	import { pushToast, reportError } from '#lib/features/notifications/store.svelte.js';
	import { network } from '#lib/utils/online.svelte.js';
	import { liveStateOf } from './live.svelte.js';
	import ConnectRemoteDialog from './ConnectRemoteDialog.svelte';
	import InterruptedDialog from './InterruptedDialog.svelte';
	import SyncSizeDialog from './SyncSizeDialog.svelte';
	import { hasInterruptedOperation, hasLargeFilesAttention, syncingKind } from './live-state';
	import { useRecoverInterrupted } from './mutations';
	import type { SyncSizeRequest } from './sync-size';
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
	const settingsQuery = useSettings(() => projectId);
	const memoQuery = useMemoSuggestion(
		() => projectId,
		() => (changesQuery.data?.length ?? 0) > 0
	);

	let tab = $state<'changes' | 'history'>('changes');
	let selectedPath = $state<string | null>(null);
	let selectedPointId = $state<string | null>('now');
	let expanded = $state<string[]>([]);
	let memo = $state('');
	// 最後に自動で入れた案。メモ欄がこれと同じ間だけ、新しい案に差し替える
	let autoMemo = '';
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
	let addSummary = $state<AddFilesSummary | null>(null);
	// 大きいファイルの確認待ち。保存のやり直しに同じメモを使うため、メモも持つ
	let sizeRequest = $state<{ memo: string; check: SizeCheck } | null>(null);
	let interruptedOpen = $state(false);
	let connectOpen = $state(false);
	// 大きいファイルのため、取り込み・アップロードを見送った内容
	let syncSize = $state<SyncSizeRequest | null>(null);

	const changes = $derived(changesQuery.data ?? []);
	const history = $derived(historyQuery.data ?? []);
	const latest = $derived(latestManualPoint(history));
	const snapshotsMode = $derived(
		settingsQuery.data?.settings.showSnapshotsInTimeline ?? 'collapsed'
	);
	const entries = $derived(buildTimeline(history, snapshotsMode));
	const selectedChange = $derived(changes.find((c) => c.path === selectedPath) ?? null);
	const selectedPoint = $derived(history.find((p) => p.id === selectedPointId) ?? null);
	const suggestion = $derived(changes.length > 0 ? (memoQuery.data ?? '') : '');
	const live = $derived(liveStateOf(projectId));
	const baseId = $derived(latest?.id ?? 'current');

	const save = useSave(
		() => projectId,
		() => {
			memo = '';
			autoMemo = '';
			sizeRequest = null;
		},
		(requestMemo, check) => (sizeRequest = { memo: requestMemo, check })
	);
	const fetchMutation = useFetch(
		() => projectId,
		() => (conflictOpen = true),
		(request) => (syncSize = request)
	);
	const push = usePush(
		() => projectId,
		(request) => (syncSize = request)
	);
	const recover = useRecoverInterrupted(
		() => projectId,
		() => (interruptedOpen = false)
	);
	const addFiles = useAddFiles(
		() => projectId,
		(outcome) => {
			const summary = summarizeAddFiles(outcome);
			if (summary.hasTooLarge) {
				tooLarge = true;
				setTimeout(() => (tooLarge = false), 4000);
			}
			if (summary.needsDialog) addSummary = summary;
		}
	);
	const restore = useRestore(
		() => projectId,
		() => (restoreRequest = null)
	);

	const syncing = $derived(
		fetchMutation.isPending ? 'fetch' : push.isPending ? 'push' : syncingKind(live.operation)
	);
	const interrupted = $derived(
		hasInterruptedOperation(live, project.data?.interruptedOperation ?? null)
	);
	// 確認のために再実行している間は、処理中の表示を優先する
	const largeFiles = $derived(hasLargeFilesAttention(live) && syncing === null);
	const attention = $derived(
		live.attention === 'auth' || live.sync === 'auth-required'
			? 'auth'
			: live.attention === 'unsaved-changes'
				? 'unsaved-changes'
				: null
	);

	$effect(() => {
		const next = nextMemo(
			untrack(() => memo),
			autoMemo,
			suggestion
		);
		autoMemo = suggestion;
		memo = next;
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
		if (text) save.mutate({ memo: text });
	}

	function chooseSize(action: SizeAction) {
		if (!sizeRequest) return;
		const choice = choiceFor(sizeRequest.check, action);
		if (choice) save.mutate({ memo: sizeRequest.memo, choice });
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

	// 選択中の保存時点の版を、読み取り専用で開く（現在のファイルは変更しない）
	async function openFileAtPoint(path: string) {
		if (!selectedPoint) return;
		try {
			await api.openFileAt(projectId, selectedPoint.id, path);
		} catch (error) {
			reportError(error);
		}
	}

	// 追加できるかどうかの検査（大きさの上限など）は、ブラウザ表示ではモック、アプリ上ではバックエンドが行う
	function submitFiles(files: DroppedFile[]) {
		if (files.length === 0) return;
		tab = 'changes';
		addFiles.mutate(files);
	}

	function onFiles(files: File[]) {
		submitFiles(files.map((f) => ({ name: f.name, size: f.size })));
	}

	async function pickFiles() {
		try {
			const paths = await api.pickFiles();
			if (paths) submitFiles(droppedFromPaths(paths));
		} catch (error) {
			reportError(error);
		}
	}

	// アプリ上では HTML のドロップが発生しないため、webview のドロップ（実パス付き）を受け取る
	$effect(() => {
		if (!isTauri) return;
		let disposed = false;
		let unlisten: (() => void) | undefined;
		void listenNativeDrop((action) => {
			if (action.kind === 'enter') dragDepth = 1;
			else if (action.kind === 'leave') dragDepth = 0;
			else {
				dragDepth = 0;
				submitFiles(action.files);
			}
		}).then((fn) => {
			if (disposed) fn();
			else unlisten = fn;
		});
		return () => {
			disposed = true;
			unlisten?.();
		};
	});

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
		hasConflict={(project.data?.hasConflict ?? false) ||
			live.attention === 'conflict' ||
			live.sync === 'conflicted'}
		{syncing}
		{attention}
		{interrupted}
		{largeFiles}
		notConnected={project.data !== undefined && !project.data.remoteConnected}
		isOnline={network.online && live.sync !== 'offline'}
		unsavedCount={changesQuery.data ? changes.length : (project.data?.unsavedCount ?? 0)}
		uploadPendingCount={project.data?.uploadPendingCount ?? 0}
		fetchPendingCount={project.data?.fetchPendingCount ?? 0}
		lastUploadedAt={project.data?.lastUploadedAt ?? null}
		onback={() => goto(resolve('/'))}
		onsettings={() => goto(`${resolve('/settings')}?project=${encodeURIComponent(projectId)}`)}
		onfetch={() => fetchMutation.mutate()}
		onpush={() => push.mutate()}
		onretry={() => fetchMutation.mutate()}
		onreview={() => (conflictOpen = true)}
		oninterrupted={() => (interruptedOpen = true)}
		onlargefiles={() => fetchMutation.mutate()}
		onconnect={() => (connectOpen = true)}
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
					onpick={isTauri ? pickFiles : undefined}
				/>
			{:else}
				<HistoryPane
					{entries}
					loading={historyQuery.isPending}
					selectedId={selectedPointId}
					{expanded}
					showAutos={snapshotsMode === 'shown'}
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
					onopenat={openFileAtPoint}
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

<SizeCheckDialog
	check={sizeRequest?.check ?? null}
	pending={save.isPending}
	onchoose={chooseSize}
	oncancel={() => (sizeRequest = null)}
/>

<InterruptedDialog
	open={interruptedOpen}
	operation={project.data?.interruptedOperation ?? null}
	recovering={recover.isPending}
	onrecover={() => recover.mutate()}
	onclose={() => (interruptedOpen = false)}
/>

<ConnectRemoteDialog
	open={connectOpen}
	{projectId}
	projectName={project.data?.name ?? ''}
	onclose={() => (connectOpen = false)}
/>

<SyncSizeDialog request={syncSize} onclose={() => (syncSize = null)} />

<AddFilesResultDialog summary={addSummary} onclose={() => (addSummary = null)} />

<ConflictModal open={conflictOpen} {projectId} onclose={() => (conflictOpen = false)} />
