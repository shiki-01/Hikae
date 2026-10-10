<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import type { FilePreview, PreviewSheet, PreviewUnavailableReason } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import { describeCellChange, groupCellChanges, hasOfficeChanges } from './office-diff';
	import { unavailableMessageKey } from './preview-view';
	import { useCompareOffice, useFilePreview } from './queries';
	import SheetGrid from './SheetGrid.svelte';
	import { mergeXlsxDiff } from './xlsx-grid';

	interface Props {
		projectId: string;
		path: string;
		/** 比べる元の時点 ID（「いま」は current） */
		from: string;
		/** 比べる先の時点 ID（「いま」は current） */
		to: string;
		onopen?: () => void;
	}

	let { projectId, path, from, to, onopen }: Props = $props();

	const office = useCompareOffice(
		() => projectId,
		() => path,
		() => from,
		() => to
	);
	// 今の版と過去の版の内容。土台のグリッドと、削除されたシートの表示に使う
	const toPreview = useFilePreview(
		() => projectId,
		() => to,
		() => path
	);
	const fromPreview = useFilePreview(
		() => projectId,
		() => from,
		() => path
	);

	/** 内容の読み取りの結果。シートがあるか、無い（ファイルが無い）か、読めない理由があるか */
	type Side =
		| { kind: 'sheets'; sheets: PreviewSheet[] }
		| { kind: 'missing' }
		| { kind: 'unavailable'; reason: PreviewUnavailableReason };

	function sideOf(preview: FilePreview | undefined): Side | null {
		if (preview === undefined) return null;
		switch (preview.kind) {
			case 'xlsx':
				return { kind: 'sheets', sheets: preview.sheets };
			case 'not_found':
				return { kind: 'missing' };
			case 'unavailable':
				return { kind: 'unavailable', reason: preview.reason };
			default:
				return { kind: 'unavailable', reason: 'unsupported_format' };
		}
	}

	const toSide = $derived(sideOf(toPreview.data));
	const fromSide = $derived(sideOf(fromPreview.data));
	const xlsx = $derived(office.data?.kind === 'xlsx' ? office.data : null);
	const loading = $derived(office.isPending || toPreview.isPending || fromPreview.isPending);
	const failed = $derived(office.isError || toPreview.isError || fromPreview.isError);
	const unavailable = $derived<PreviewUnavailableReason | null>(
		toSide?.kind === 'unavailable'
			? toSide.reason
			: fromSide?.kind === 'unavailable'
				? fromSide.reason
				: office.data?.kind === 'unavailable'
					? office.data.reason
					: null
	);

	// 今の版が無い（削除）なら過去の版の全セルを削除にし、過去の版が無い（新規）なら今の版の全セルを追加にする
	const sheets = $derived(
		mergeXlsxDiff({
			to: toSide?.kind === 'sheets' ? toSide.sheets : null,
			from: fromSide?.kind === 'sheets' ? fromSide.sheets : null,
			addedSheets: xlsx?.addedSheets ?? [],
			removedSheets: xlsx?.removedSheets ?? [],
			cells: xlsx?.cells ?? []
		})
	);

	const changed = $derived(xlsx !== null && hasOfficeChanges(xlsx));
	const groups = $derived(xlsx ? groupCellChanges(xlsx.cells) : []);
	const count = $derived(
		xlsx ? xlsx.addedSheets.length + xlsx.removedSheets.length + xlsx.cells.length : 0
	);

	const cellPalette = (before: string | null, after: string | null): string => {
		if (before !== null && after !== null) return 'bg:diff-mod-bg fg:fg';
		return after !== null ? 'bg:diff-add-bg fg:diff-add-fg' : 'bg:diff-del-bg fg:diff-del-fg';
	};
	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg" data-testid="xlsx-diff">
	{#if loading}
		<div class="flex align-items:center justify-content:center h:100%"><Spinner /></div>
	{:else if failed}
		<div class={centered}>
			<p class="m:0 type-body fg:fg-muted text-align:center">{t('compare.office.failed')}</p>
		</div>
	{:else if unavailable !== null}
		<div class={centered}>
			<p class="m:0 type-body text-align:center">{t(unavailableMessageKey(unavailable))}</p>
			<p class="m:0 type-small fg:fg-muted text-align:center">{t('preview.unavailable.safe')}</p>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			{/if}
		</div>
	{:else}
		<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">{t('grid.changes_note')}</p>
		<div class="flex flex-direction:column flex:1 min-h:0 min-w:0">
			<SheetGrid {sheets} ariaLabel={path} />
		</div>
		<details class="flex-shrink:0 max-h:40% overflow:auto bt:1px|solid|border">
			<summary class="px:4 py:2 type-small cursor:pointer">
				{t('diff.xlsx.changes', { count })}
			</summary>
			{#if xlsx && !changed}
				<p class="m:0 px:4 py:2 type-small fg:fg-muted">{t('diff.no_changes')}</p>
			{:else if xlsx}
				{#each xlsx.addedSheets as name (name)}
					<div class="px:4 py:1 type-small bg:diff-add-bg fg:diff-add-fg">
						{t('compare.office.sheet_added', { name })}
					</div>
				{/each}
				{#each xlsx.removedSheets as name (name)}
					<div class="px:4 py:1 type-small bg:diff-del-bg fg:diff-del-fg">
						{t('compare.office.sheet_removed', { name })}
					</div>
				{/each}
				{#each groups as group (group.sheet)}
					<h3 class="m:0 px:4 pt:3 pb:1 type-small fg:fg">{group.sheet}</h3>
					{#each group.changes as change, index (index)}
						{@const line = describeCellChange(change)}
						<div class={`px:4 py:1 type-mono ${cellPalette(change.before, change.after)}`}>
							{t(line.key, line.params)}
						</div>
					{/each}
				{/each}
				{#if xlsx.truncated > 0}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted">
						{t('compare.office.more', { count: xlsx.truncated })}
					</p>
				{/if}
			{/if}
		</details>
	{/if}
</div>
