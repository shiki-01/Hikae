<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { nextIndex } from '#lib/utils/keyboard.js';
	import {
		cellKey,
		cellRef,
		columnLabel,
		gridSize,
		toCellMap,
		type CellTone,
		type GridCell,
		type XlsxSheetView
	} from '#lib/features/compare/xlsx-grid.js';

	interface Props {
		/** 表示するシート（タブの順） */
		sheets: XlsxSheetView[];
		/** 表の名前（支援技術向け） */
		ariaLabel: string;
	}

	let { sheets, ariaLabel }: Props = $props();

	/** 行は段階的に増やす（大きいシートで一度に大量の要素を作って固まらないように） */
	const ROW_CHUNK = 200;

	const base = $props.id();
	let active = $state(0);
	let tabButtons = $state<HTMLButtonElement[]>([]);
	let scroller = $state<HTMLDivElement>();
	let sentinel = $state<HTMLTableRowElement>();
	let shown = $state(ROW_CHUNK);

	const index = $derived(Math.min(active, Math.max(sheets.length - 1, 0)));
	const current = $derived(sheets[index] ?? null);
	const size = $derived(gridSize(current?.rows ?? 0, current?.cols ?? 0));
	const cells = $derived(toCellMap(current?.cells ?? []));
	const rowIndexes = $derived(Array.from({ length: Math.min(shown, size.rows) }, (_, i) => i));
	const colIndexes = $derived(Array.from({ length: size.cols }, (_, i) => i));

	// 別のシートに切り替えたら、行の表示を最初からやり直す
	$effect(() => {
		void current;
		shown = ROW_CHUNK;
	});

	// 末尾の目印が見えたら、行を増やす。増えた後も見えていれば続けて増やす
	$effect(() => {
		void shown;
		const target = sentinel;
		const root = scroller;
		if (!target || !root) return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries.some((entry) => entry.isIntersecting)) {
					shown = Math.min(size.rows, shown + ROW_CHUNK);
				}
			},
			{ root, rootMargin: '200px 0px' }
		);
		observer.observe(target);
		return () => observer.disconnect();
	});

	const tonePalette: Record<CellTone, string> = {
		changed: 'bg:diff-mod-bg fg:fg',
		added: 'bg:diff-add-bg fg:diff-add-fg',
		removed: 'bg:diff-del-bg fg:diff-del-fg'
	};

	/** 強調したセルの説明（変更前 → 変更後）。強調の無いセルは値そのもの */
	function titleOf(row: number, col: number, cell: GridCell): string {
		const ref = cellRef(row, col);
		switch (cell.tone) {
			case 'changed':
				return t('compare.office.cell_changed', {
					ref,
					before: cell.before ?? '',
					after: cell.value
				});
			case 'added':
				return t('compare.office.cell_added', { ref, value: cell.value });
			case 'removed':
				return t('compare.office.cell_removed', { ref, value: cell.before ?? '' });
			default:
				return cell.value;
		}
	}

	function select(next: number) {
		active = next;
	}

	function onTabKey(event: KeyboardEvent, position: number) {
		const target = nextIndex(event.key, position, sheets.length, 'horizontal');
		if (target === null) return;
		event.preventDefault();
		select(target);
		tabButtons[target]?.focus();
	}
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg">
	{#if sheets.length === 0}
		<p class="m:0 p:6 type-body fg:fg-muted text-align:center">{t('grid.no_sheets')}</p>
	{:else}
		<div
			role="tablist"
			aria-label={t('grid.tabs')}
			class="flex gap:1 px:2 py:1 bb:1px|solid|border overflow-x:auto flex-shrink:0"
		>
			{#each sheets as sheet, position (position)}
				{@const selected = position === index}
				<button
					bind:this={tabButtons[position]}
					type="button"
					role="tab"
					id={`${base}-tab-${position}`}
					aria-selected={selected}
					aria-controls={`${base}-panel`}
					tabindex={selected ? 0 : -1}
					onclick={() => select(position)}
					onkeydown={(event) => onTabKey(event, position)}
					class={`flex align-items:center gap:2 px:3 py:1 type-small white-space:nowrap b:0 r:md cursor:pointer flex-shrink:0 ${
						selected
							? 'bg:accent-subtle fg:fg font-weight:700'
							: 'bg:transparent fg:fg-muted fg:fg:hover bg:bg-subtle:hover'
					}`}
				>
					{sheet.name}
					{#if sheet.badge}
						<span class="px:1 r:sm br:1px|solid|border type-small">
							{t(sheet.badge === 'added' ? 'grid.badge_added' : 'grid.badge_removed')}
						</span>
					{/if}
				</button>
			{/each}
		</div>
		<div
			role="tabpanel"
			id={`${base}-panel`}
			aria-labelledby={`${base}-tab-${index}`}
			aria-label={ariaLabel}
			class="flex flex-direction:column flex:1 min-h:0 min-w:0"
		>
			{#if current}
				{#if size.limitedRows}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
						{t('grid.limited_rows', { count: size.rows })}
					</p>
				{/if}
				{#if size.limitedCols}
					<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
						{t('grid.limited_cols', { count: size.cols })}
					</p>
				{/if}
				{#if size.rows === 0 || size.cols === 0}
					<p class="m:0 p:6 type-body fg:fg-muted text-align:center">{t('grid.empty')}</p>
				{:else}
					<div bind:this={scroller} class="flex:1 min-h:0 overflow:auto">
						<table
							class="border-collapse:collapse table-layout:fixed"
							aria-label={t('grid.table', { name: current.name })}
						>
							<colgroup>
								<col class="w:48px" />
								{#each colIndexes as col (col)}
									<col class="w:120px" />
								{/each}
							</colgroup>
							<thead>
								<tr>
									<th scope="col" class="sticky top:0 left:0 z:2 bg:bg-subtle bb:1px|solid|border"
									></th>
									{#each colIndexes as col (col)}
										<th
											scope="col"
											class="sticky top:0 z:1 bg:bg-subtle px:2 py:1 type-small fg:fg-muted text-align:center bb:1px|solid|border"
										>
											{columnLabel(col)}
										</th>
									{/each}
								</tr>
							</thead>
							<tbody>
								{#each rowIndexes as row (row)}
									<tr>
										<th
											scope="row"
											class="sticky left:0 z:1 bg:bg-subtle px:2 type-small fg:fg-muted text-align:right br:1px|solid|border"
										>
											{row + 1}
										</th>
										{#each colIndexes as col (col)}
											{@const cell = cells.get(cellKey(row, col))}
											<td
												class={`w:120px max-w:120px px:2 type-mono overflow:hidden text-overflow:ellipsis white-space:nowrap bb:1px|solid|border br:1px|solid|border ${
													cell?.tone ? tonePalette[cell.tone] : ''
												}`}
												title={cell ? titleOf(row, col, cell) : undefined}
												aria-label={cell?.tone ? titleOf(row, col, cell) : undefined}
												tabindex={cell?.tone ? 0 : undefined}
											>
												{cell?.value ?? ''}
											</td>
										{/each}
									</tr>
								{/each}
								{#if shown < size.rows}
									<tr bind:this={sentinel}>
										<td colspan={size.cols + 1} class="p:2 type-small fg:fg-muted"></td>
									</tr>
								{/if}
							</tbody>
						</table>
					</div>
				{/if}
			{/if}
		</div>
	{/if}
</div>
