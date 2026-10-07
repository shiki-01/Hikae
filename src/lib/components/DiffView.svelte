<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { formatBytes, formatDateTime } from '#lib/i18n/format.js';
	import type { DiffRow, FileDiff } from '#lib/api/types.js';
	import Button from './Button.svelte';
	import Spinner from './Spinner.svelte';
	import { annotateBlocks, pairRows } from './diff-rows';

	interface Props {
		diff: FileDiff | null;
		loading?: boolean;
		mode?: 'split' | 'unified';
		activeBlock?: number | null;
		leftLabel?: string;
		rightLabel?: string;
		emptyText?: string;
		onopen?: () => void;
		class?: string;
	}

	let {
		diff,
		loading = false,
		mode = 'split',
		activeBlock = null,
		leftLabel = '',
		rightLabel = '',
		emptyText = t('diff.select_file'),
		onopen,
		class: className = ''
	}: Props = $props();

	let scroller = $state<HTMLDivElement>();
	let spinnerVisible = $state(false);

	$effect(() => {
		if (!loading) {
			spinnerVisible = false;
			return;
		}
		const timer = setTimeout(() => (spinnerVisible = true), 300);
		return () => clearTimeout(timer);
	});

	$effect(() => {
		if (activeBlock === null || !scroller) return;
		scroller
			.querySelector(`[data-block="${activeBlock}"]`)
			?.scrollIntoView({ block: 'center', behavior: 'auto' });
	});

	const rows = $derived(diff?.kind === 'text' ? diff.rows : []);
	const split = $derived(pairRows(rows));
	const unified = $derived(annotateBlocks(rows));
	const splitBlocks = $derived(split.map((s) => s.block));
	const unifiedBlocks = $derived(unified.map((u) => u.block));

	const palette = {
		add: 'bg:diff-add-bg fg:diff-add-fg',
		del: 'bg:diff-del-bg fg:diff-del-fg',
		context: 'fg:fg',
		none: 'bg:bg-subtle'
	};
	const sign = { add: '+', del: '-', context: '' };

	function kindOf(row: DiffRow | null): 'add' | 'del' | 'context' | 'none' {
		return row ? row.kind : 'none';
	}

	function marker(block: number | null) {
		return block !== null && block === activeBlock
			? 'bl:3px|solid|accent'
			: 'bl:3px|solid|transparent';
	}

	function firstOfBlock(blocks: (number | null)[], index: number) {
		const block = blocks[index];
		return block !== null && blocks[index - 1] !== block ? block : undefined;
	}

	const numberCell = 'w:48px px:2 text-align:right fg:fg-muted type-mono user-select:none';
	const signCell = 'w:20px text-align:center type-mono';
	const textCell = 'px:2 type-mono white-space:pre-wrap overflow-wrap:anywhere';
</script>

<div
	bind:this={scroller}
	class={`flex:1 min-h:0 min-w:0 overflow:auto bg:bg ${className}`}
	data-testid="diff-view"
>
	{#if loading && spinnerVisible}
		<div class="flex align-items:center justify-content:center h:100%"><Spinner /></div>
	{:else if loading}
		<div class="h:100%"></div>
	{:else if !diff}
		<div class="flex align-items:center justify-content:center h:100% p:6">
			<p class="m:0 type-body fg:fg-muted">{emptyText}</p>
		</div>
	{:else if diff.kind === 'identical'}
		<div class="flex align-items:center justify-content:center h:100% p:6">
			<p class="m:0 type-body fg:fg-muted">{t('diff.identical')}</p>
		</div>
	{:else if diff.kind === 'too_large'}
		<div
			class="flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6"
		>
			<p class="m:0 type-body">{t('diff.too_large', { size: formatBytes(diff.sizeBytes) })}</p>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			{/if}
		</div>
	{:else if diff.kind === 'info'}
		<div
			class="flex flex-direction:column gap:4 align-items:center justify-content:center h:100% p:6"
		>
			<p class="m:0 type-body fg:fg-muted">{t('diff.no_preview')}</p>
			<dl class="m:0 grid grid-template-columns:auto|auto gap:2 type-body">
				<dt class="fg:fg-muted">{t('diff.file_type')}</dt>
				<dd class="m:0">{t(`file_kind.${diff.fileKind}`)}</dd>
				<dt class="fg:fg-muted">{t('diff.file_size')}</dt>
				<dd class="m:0">{formatBytes(diff.sizeBytes)}</dd>
				<dt class="fg:fg-muted">{t('diff.modified_at')}</dt>
				<dd class="m:0">{formatDateTime(diff.modifiedAt)}</dd>
			</dl>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			{/if}
		</div>
	{:else if mode === 'split'}
		<table class="w:100% border-collapse:collapse table-layout:fixed" aria-label={t('diff.table')}>
			<colgroup>
				<col class="w:48px" />
				<col class="w:20px" />
				<col />
				<col class="w:48px" />
				<col class="w:20px" />
				<col />
			</colgroup>
			{#if leftLabel || rightLabel}
				<thead class="sticky top:0 bg:bg-subtle">
					<tr>
						<th colspan="3" scope="col" class="px:3 py:1 text-align:left type-small fg:fg">
							{leftLabel}
						</th>
						<th
							colspan="3"
							scope="col"
							class="px:3 py:1 text-align:left type-small fg:fg bl:1px|solid|border"
						>
							{rightLabel}
						</th>
					</tr>
				</thead>
			{/if}
			<tbody>
				{#each split as item, index (index)}
					{@const left = kindOf(item.left)}
					{@const right = kindOf(item.right)}
					<tr data-block={firstOfBlock(splitBlocks, index)}>
						<td class={`${numberCell} ${marker(item.block)} ${palette[left]}`}>
							{item.left?.oldNo ?? ''}
						</td>
						<td class={`${signCell} ${palette[left]}`}>{left === 'del' ? sign.del : ''}</td>
						<td class={`${textCell} ${palette[left]}`}>{item.left?.text ?? ''}</td>
						<td class={`${numberCell} bl:1px|solid|border ${palette[right]}`}>
							{item.right?.newNo ?? ''}
						</td>
						<td class={`${signCell} ${palette[right]}`}>{right === 'add' ? sign.add : ''}</td>
						<td class={`${textCell} ${palette[right]}`}>{item.right?.text ?? ''}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{:else}
		<table class="w:100% border-collapse:collapse table-layout:fixed" aria-label={t('diff.table')}>
			<colgroup>
				<col class="w:48px" />
				<col class="w:48px" />
				<col class="w:20px" />
				<col />
			</colgroup>
			<tbody>
				{#each unified as item, index (index)}
					<tr data-block={firstOfBlock(unifiedBlocks, index)}>
						<td class={`${numberCell} ${marker(item.block)} ${palette[item.row.kind]}`}>
							{item.row.oldNo ?? ''}
						</td>
						<td class={`${numberCell} ${palette[item.row.kind]}`}>{item.row.newNo ?? ''}</td>
						<td class={`${signCell} ${palette[item.row.kind]}`}>{sign[item.row.kind]}</td>
						<td class={`${textCell} ${palette[item.row.kind]}`}>{item.row.text}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>
