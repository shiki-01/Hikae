<script lang="ts">
	// 比較の表示の本体（iframe の中）。差分の種類ごとに描く。ホストとは props（データ）でだけつながる（設計書 11.7）。
	// 画像は CompareImage、テキストは DiffView、docx・xlsx・注記・比べられないもの、はここで描く。
	import { t } from '#lib/i18n/index.js';
	import { formatBytes, formatDateTime } from '#lib/i18n/format.js';
	import Button from '#lib/components/Button.svelte';
	import {
		describeCellChange,
		groupCellChanges,
		hasOfficeChanges
	} from '#lib/features/compare/office-diff.js';
	import { unavailableMessageKey } from '#lib/features/compare/preview-view.js';
	import { mergeXlsxDiff } from '#lib/features/compare/xlsx-grid.js';
	import type {
		CompareOptions,
		CompareSide,
		DiffPayload,
		FileInfo,
		RenderContent
	} from '#lib/features/extensions/renderer-protocol.js';
	import CompareImage from './CompareImage.svelte';
	import DiffView from './DiffView.svelte';
	import SheetGrid from './SheetGrid.svelte';

	interface Props {
		left: CompareSide;
		right: CompareSide;
		/** 差分の結果。無いときは画像の比較 */
		diff: DiffPayload | undefined;
		options: CompareOptions;
		/** 表示できなかったときに、ホストへ知らせる */
		onfail: (code: 'render-failed' | 'unsupported') => void;
		/** 「ファイルを開く」の要求 */
		onopen: () => void;
	}

	let { left, right, diff, options, onfail, onopen }: Props = $props();

	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';
	const paragraphPalette = {
		context: 'fg:fg',
		add: 'bg:diff-add-bg fg:diff-add-fg',
		del: 'bg:diff-del-bg fg:diff-del-fg'
	};
	const paragraphSign = { context: '', add: '+', del: '-' };
	const numberCell =
		'w:48px px:2 text-align:right fg:fg-muted type-mono user-select:none flex-shrink:0';
	const signCell = 'w:20px text-align:center type-mono flex-shrink:0';
	const textCell = 'flex:1 min-w:0 px:2 type-mono white-space:pre-wrap overflow-wrap:anywhere';

	/** セルの差分の色。変更は黄、追加は緑、削除は赤（既存の差分の色に合わせる） */
	function cellPalette(before: string | null, after: string | null): string {
		if (before !== null && after !== null) return 'bg:diff-mod-bg fg:fg';
		return after !== null ? paragraphPalette.add : paragraphPalette.del;
	}

	/** 内容がシートなら、そのシートの一覧（無い・読めなかったら null） */
	function sheetsOf(side: CompareSide) {
		const content: RenderContent | undefined = side.content;
		return content?.kind === 'cells' ? content.sheets : null;
	}

	/** 画面の表記の更新日時（ミリ秒。無ければ表示しない） */
	function modifiedText(info: FileInfo): string | null {
		return info.modifiedAt === null ? null : formatDateTime(new Date(info.modifiedAt));
	}

	const xlsx = $derived(diff?.kind === 'cells' ? diff : null);
	const xlsxSheets = $derived(
		xlsx
			? mergeXlsxDiff({
					to: sheetsOf(right),
					from: sheetsOf(left),
					addedSheets: xlsx.addedSheets,
					removedSheets: xlsx.removedSheets,
					cells: xlsx.cells
				})
			: []
	);
	const xlsxChanged = $derived(
		xlsx !== null &&
			hasOfficeChanges({
				kind: 'xlsx',
				addedSheets: xlsx.addedSheets,
				removedSheets: xlsx.removedSheets,
				cells: xlsx.cells,
				truncated: xlsx.truncated
			})
	);
	const xlsxGroups = $derived(xlsx ? groupCellChanges(xlsx.cells) : []);
	const xlsxCount = $derived(
		xlsx ? xlsx.addedSheets.length + xlsx.removedSheets.length + xlsx.cells.length : 0
	);
	const docxChanged = $derived(
		diff?.kind === 'paragraphs' && hasOfficeChanges({ kind: 'docx', rows: diff.rows, truncated: 0 })
	);
</script>

<svelte:boundary onerror={() => onfail('render-failed')}>
	<div
		class="flex flex-direction:column flex:1 min-h:0 min-w:0 h:100% bg:bg"
		aria-label={right.label}
	>
		{#if diff === undefined}
			<CompareImage {left} {right} />
		{:else if diff.kind === 'lines'}
			<DiffView
				rows={diff.rows}
				mode={options.viewMode}
				activeBlock={options.activeBlock}
				leftLabel={left.label}
				rightLabel={right.label}
			/>
		{:else if diff.kind === 'identical'}
			<div class={centered}>
				<p class="m:0 type-body fg:fg-muted text-align:center">{t('diff.identical')}</p>
			</div>
		{:else if diff.kind === 'new_empty'}
			<div class={centered}>
				<p class="m:0 type-body fg:fg-muted text-align:center">{t('diff.new_empty')}</p>
			</div>
		{:else if diff.kind === 'too_large'}
			<div class={centered}>
				<p class="m:0 type-body text-align:center">
					{t('diff.too_large', { size: formatBytes(diff.sizeBytes) })}
				</p>
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			</div>
		{:else if diff.kind === 'unavailable'}
			<div class={centered}>
				<p class="m:0 type-body text-align:center">{t(unavailableMessageKey(diff.reason))}</p>
				<p class="m:0 type-small fg:fg-muted text-align:center">
					{t(diff.info ? 'compare.office.unavailable.safe' : 'preview.unavailable.safe')}
				</p>
				{#if diff.info}
					<dl class="m:0 grid grid-template-columns:auto|auto gap:2 type-body">
						<dt class="fg:fg-muted">{t('diff.file_type')}</dt>
						<dd class="m:0">{t(`file_kind.${diff.info.fileKind}`)}</dd>
						<dt class="fg:fg-muted">{t('diff.file_size')}</dt>
						<dd class="m:0">{formatBytes(diff.info.sizeBytes)}</dd>
						{#if modifiedText(diff.info)}
							<dt class="fg:fg-muted">{t('diff.modified_at')}</dt>
							<dd class="m:0">{modifiedText(diff.info)}</dd>
						{/if}
					</dl>
				{/if}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			</div>
		{:else if diff.kind === 'paragraphs'}
			<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
				{t('compare.office.format_note')}
			</p>
			<div class="flex:1 min-h:0 overflow:auto">
				{#if !docxChanged}
					<div class="flex align-items:center justify-content:center h:100% p:6">
						<p class="m:0 type-body fg:fg-muted">{t('diff.identical')}</p>
					</div>
				{:else}
					{#each diff.rows as row, index (index)}
						{#if row.kind === 'omitted'}
							<p class="m:0 px:4 py:1 type-small fg:fg-muted">
								{t('compare.office.omitted', { count: row.count })}
							</p>
						{:else}
							<div class={`flex align-items:flex-start ${paragraphPalette[row.kind]}`}>
								<span class={numberCell}>{row.kind === 'del' ? row.oldNo : row.newNo}</span>
								<span class={signCell}>{paragraphSign[row.kind]}</span>
								<span class={textCell}>{row.text}</span>
							</div>
						{/if}
					{/each}
					{#if diff.truncated > 0}
						<p class="m:0 px:4 py:2 type-small fg:fg-muted">
							{t('compare.office.more', { count: diff.truncated })}
						</p>
					{/if}
				{/if}
			</div>
		{:else if diff.kind === 'cells'}
			<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
				{t('grid.changes_note')}
			</p>
			<div class="flex flex-direction:column flex:1 min-h:0 min-w:0">
				<SheetGrid sheets={xlsxSheets} ariaLabel={right.file.name} />
			</div>
			<details class="flex-shrink:0 max-h:40% overflow:auto bt:1px|solid|border">
				<summary class="px:4 py:2 type-small cursor:pointer">
					{t('diff.xlsx.changes', { count: xlsxCount })}
				</summary>
				{#if !xlsxChanged}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted">{t('diff.no_changes')}</p>
				{:else}
					{#each diff.addedSheets as name (name)}
						<div class="px:4 py:1 type-small bg:diff-add-bg fg:diff-add-fg">
							{t('compare.office.sheet_added', { name })}
						</div>
					{/each}
					{#each diff.removedSheets as name (name)}
						<div class="px:4 py:1 type-small bg:diff-del-bg fg:diff-del-fg">
							{t('compare.office.sheet_removed', { name })}
						</div>
					{/each}
					{#each xlsxGroups as group (group.sheet)}
						<h3 class="m:0 px:4 pt:3 pb:1 type-small fg:fg">{group.sheet}</h3>
						{#each group.changes as change, index (index)}
							{@const line = describeCellChange(change)}
							<div class={`px:4 py:1 type-mono ${cellPalette(change.before, change.after)}`}>
								{t(line.key, line.params)}
							</div>
						{/each}
					{/each}
					{#if diff.truncated > 0}
						<p class="m:0 px:4 py:2 type-small fg:fg-muted">
							{t('compare.office.more', { count: diff.truncated })}
						</p>
					{/if}
				{/if}
			</details>
		{/if}
	</div>
</svelte:boundary>
