<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import {
		pickText,
		type ConflictDocument,
		type HunkPick,
		type HunkSegment
	} from './conflict-markers';

	interface Props {
		doc: ConflictDocument;
		picks: ReadonlyArray<HunkPick | null>;
		onpick: (index: number, pick: HunkPick) => void;
		onclose: () => void;
	}

	let { doc, picks, onpick, onclose }: Props = $props();

	/** 行の並び。共通部分は 3 ペインに同じ内容を出し、競合ハンクには 0 から番号を振る */
	type Row = { kind: 'common'; text: string } | { kind: 'hunk'; index: number; hunk: HunkSegment };

	const rows = $derived.by<Row[]>(() => {
		let index = 0;
		return doc.segments.map((segment): Row => {
			if (segment.kind === 'common') return { kind: 'common', text: segment.text };
			const row: Row = { kind: 'hunk', index, hunk: segment };
			index += 1;
			return row;
		});
	});

	const remaining = $derived(picks.filter((pick) => pick === null).length);

	function choose(index: number, kind: 'mine' | 'theirs' | 'both' | 'edited', hunk: HunkSegment) {
		const current = picks[index];
		if (kind === 'mine') onpick(index, { kind: 'mine' });
		else if (kind === 'theirs') onpick(index, { kind: 'theirs' });
		else if (kind === 'both') onpick(index, { kind: 'both' });
		else
			onpick(index, {
				kind: 'edited',
				text: current?.kind === 'edited' ? current.text : hunk.mine
			});
	}

	function isPicked(pick: HunkPick | null | undefined, kind: HunkPick['kind']): boolean {
		return pick?.kind === kind;
	}

	const paneClass =
		'm:0 p:2 r:sm bg:bg-subtle type-mono white-space:pre-wrap overflow-wrap:anywhere min-width:0';
</script>

<section
	aria-label={t('conflict.line_title')}
	class="flex flex-direction:column gap:3 p:4 r:md bg:bg-raised b:1px|solid|border min-width:0"
>
	<div class="flex align-items:center justify-content:space-between gap:3">
		<h4 class="m:0 type-heading">{t('conflict.line_title')}</h4>
		<Button size="sm" variant="ghost" onclick={onclose}>{t('conflict.line_close')}</Button>
	</div>

	<div class="grid grid-template-columns:repeat(3,minmax(0,1fr)) gap:2 type-small fg:fg-muted">
		<span>{t('conflict.this_pc')}</span>
		<span>{t('conflict.cloud')}</span>
		<span>{t('conflict.line_result')}</span>
	</div>

	{#each rows as row, i (i)}
		{#if row.kind === 'common'}
			<div class="grid grid-template-columns:repeat(3,minmax(0,1fr)) gap:2 align-items:start">
				<pre class={paneClass}>{row.text}</pre>
				<pre class={paneClass}>{row.text}</pre>
				<pre class={paneClass}>{row.text}</pre>
			</div>
		{:else}
			{@const pick = picks[row.index]}
			<div class="flex flex-direction:column gap:2 p:2 r:sm b:1px|solid|border">
				<h5 class="m:0 type-small font-weight:500">
					{t('conflict.line_hunk', { n: row.index + 1 })}
				</h5>
				<div class="grid grid-template-columns:repeat(3,minmax(0,1fr)) gap:2 align-items:start">
					<pre class={paneClass}>{row.hunk.mine === ''
							? t('conflict.line_empty')
							: row.hunk.mine}</pre>
					<pre class={paneClass}>{row.hunk.theirs === ''
							? t('conflict.line_empty')
							: row.hunk.theirs}</pre>
					<div class="flex flex-direction:column gap:2 min-width:0">
						<div class="flex flex-flow:row|wrap gap:1">
							<Button
								size="sm"
								variant={isPicked(pick, 'mine') ? 'primary' : 'secondary'}
								onclick={() => choose(row.index, 'mine', row.hunk)}
							>
								{t('conflict.this_pc')}
							</Button>
							<Button
								size="sm"
								variant={isPicked(pick, 'theirs') ? 'primary' : 'secondary'}
								onclick={() => choose(row.index, 'theirs', row.hunk)}
							>
								{t('conflict.cloud')}
							</Button>
							<Button
								size="sm"
								variant={isPicked(pick, 'both') ? 'primary' : 'secondary'}
								onclick={() => choose(row.index, 'both', row.hunk)}
							>
								{t('conflict.line_pick_both')}
							</Button>
							<Button
								size="sm"
								variant={isPicked(pick, 'edited') ? 'primary' : 'secondary'}
								onclick={() => choose(row.index, 'edited', row.hunk)}
							>
								{t('conflict.line_pick_edit')}
							</Button>
						</div>
						{#if pick?.kind === 'both'}
							<p class="m:0 type-small fg:fg-muted">{t('conflict.line_both_hint')}</p>
						{/if}
						{#if pick?.kind === 'edited'}
							<TextField
								multiline
								rows={6}
								maxRows={12}
								value={pick.text}
								ariaLabel={t('conflict.line_edit_label', { n: row.index + 1 })}
								oninput={(text) => onpick(row.index, { kind: 'edited', text })}
							/>
						{:else if pick}
							<pre class={`${paneClass} bg:accent-subtle`}>{pickText(row.hunk, pick)}</pre>
						{:else}
							<p class="m:0 type-small fg:fg-muted">{t('conflict.line_unpicked')}</p>
						{/if}
					</div>
				</div>
			</div>
		{/if}
	{/each}

	<p class="m:0 type-body fg:fg-muted" aria-live="polite">
		{remaining > 0 ? t('conflict.line_remaining', { count: remaining }) : t('conflict.line_done')}
	</p>
</section>
