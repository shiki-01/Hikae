<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import DiffView from '#lib/components/DiffView.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import {
		describeCellChange,
		groupCellChanges,
		hasOfficeChanges,
		unavailableMessageKey
	} from './office-diff';
	import { useCompare, useCompareOffice } from './queries';

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
	const data = $derived(office.data ?? null);
	const unavailable = $derived(data?.kind === 'unavailable' ? data.reason : null);

	// 抽出できなかったときだけ、従来のファイル情報（種類・サイズ・更新日時）を取る
	const info = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to,
		() => unavailable !== null
	);

	const changed = $derived(data !== null && data.kind !== 'unavailable' && hasOfficeChanges(data));
	const groups = $derived(data?.kind === 'xlsx' ? groupCellChanges(data.cells) : []);

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
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg" data-testid="office-compare">
	{#if office.isPending}
		<div class="flex align-items:center justify-content:center h:100%"><Spinner /></div>
	{:else if data === null || office.isError}
		<div class="flex align-items:center justify-content:center h:100% p:6">
			<p class="m:0 type-body fg:fg-muted">{t('compare.office.failed')}</p>
		</div>
	{:else if data.kind === 'unavailable'}
		<p class="m:0 px:4 py:2 type-small bb:1px|solid|border">
			{t(unavailableMessageKey(data.reason))}{t('compare.office.unavailable.safe')}
		</p>
		<DiffView class="flex:1" diff={info.data ?? null} loading={info.isPending} {onopen} />
	{:else}
		<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
			{t('compare.office.format_note')}
		</p>
		<div class="flex:1 min-h:0 overflow:auto">
			{#if !changed}
				<div class="flex align-items:center justify-content:center h:100% p:6">
					<p class="m:0 type-body fg:fg-muted">{t('diff.identical')}</p>
				</div>
			{:else if data.kind === 'docx'}
				{#each data.rows as row, index (index)}
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
				{#if data.truncated > 0}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted">
						{t('compare.office.more', { count: data.truncated })}
					</p>
				{/if}
			{:else if data.kind === 'xlsx'}
				{#each data.addedSheets as name (name)}
					<div class={`px:4 py:1 type-small ${paragraphPalette.add}`}>
						{t('compare.office.sheet_added', { name })}
					</div>
				{/each}
				{#each data.removedSheets as name (name)}
					<div class={`px:4 py:1 type-small ${paragraphPalette.del}`}>
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
				{#if data.truncated > 0}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted">
						{t('compare.office.more', { count: data.truncated })}
					</p>
				{/if}
			{/if}
		</div>
	{/if}
</div>
