<script lang="ts">
	// テキストの行差分の表（並べる・重ねる）。表示拡張（iframe）の中で描く（設計書 11.7）。
	// 差分の計算はホストが行い、ここでは受け取った行を描くだけ。
	import { t } from '#lib/i18n/index.js';
	import type { DiffRow } from '#lib/api/types.js';
	import { annotateBlocks, pairRows } from '#lib/components/diff-rows.js';

	interface Props {
		rows: DiffRow[];
		mode?: 'split' | 'unified';
		/** 強調する箇所の番号（この箇所へスクロールする） */
		activeBlock?: number | null;
		leftLabel?: string;
		rightLabel?: string;
	}

	let {
		rows,
		mode = 'split',
		activeBlock = null,
		leftLabel = '',
		rightLabel = ''
	}: Props = $props();

	let scroller = $state<HTMLDivElement>();

	$effect(() => {
		if (activeBlock === null || !scroller) return;
		scroller
			.querySelector(`[data-block="${activeBlock}"]`)
			?.scrollIntoView({ block: 'center', behavior: 'auto' });
	});

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
	class="flex:1 min-h:0 min-w:0 overflow:auto bg:bg"
	data-testid="diff-view"
>
	{#if mode === 'split'}
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
