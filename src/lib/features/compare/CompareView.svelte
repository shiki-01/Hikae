<script lang="ts">
	import { ArrowLeftRight, ChevronDown, ChevronUp, X } from '@lucide/svelte';
	import { onMount, untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import DiffView from '#lib/components/DiffView.svelte';
	import { countBlocks } from '#lib/components/diff-rows.js';
	import IconButton from '#lib/components/IconButton.svelte';
	import Select, { type SelectOption } from '#lib/components/Select.svelte';
	import SegmentedControl from '#lib/components/SegmentedControl.svelte';
	import { canStep, stepBlock, swapPoints, type StepDirection } from './navigation';
	import { useCompare } from './queries';

	interface Props {
		projectId: string;
		path: string;
		points: SelectOption[];
		initialFrom: string;
		initialTo: string;
		onclose: () => void;
		onopen?: () => void;
	}

	let { projectId, path, points, initialFrom, initialTo, onclose, onopen }: Props = $props();

	let root = $state<HTMLElement>();
	let from = $state(untrack(() => initialFrom));
	let to = $state(untrack(() => initialTo));
	let mode = $state<'split' | 'unified'>('split');

	const diff = useCompare(
		() => projectId,
		() => path,
		() => from,
		() => to
	);

	const total = $derived(diff.data?.kind === 'text' ? countBlocks(diff.data.rows) : 0);
	const labelOf = (id: string) => points.find((p) => p.value === id)?.label ?? '';

	let active = $derived<number | null>(total > 0 ? 0 : null);

	onMount(() => {
		const opener = document.activeElement as HTMLElement | null;
		root?.focus();
		return () => opener?.focus?.();
	});

	function onKeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape' || event.defaultPrevented) return;
		event.preventDefault();
		event.stopPropagation();
		onclose();
	}

	function step(direction: StepDirection) {
		active = stepBlock(active, total, direction);
	}

	function swap() {
		const swapped = swapPoints(from, to);
		from = swapped.from;
		to = swapped.to;
	}
</script>

<svelte:window onkeydown={onKeydown} />

<section
	bind:this={root}
	tabindex="-1"
	aria-label={t('compare.title', { name: path })}
	class="position:absolute inset:0 z:20 flex flex-direction:column bg:bg"
>
	<header class="flex flex-direction:column gap:3 px:4 py:3 bb:1px|solid|border bg:bg-subtle">
		<div class="flex align-items:center justify-content:space-between gap:3">
			<h2 class="m:0 type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap">
				{path}
			</h2>
			<div class="flex align-items:center gap:2 flex-shrink:0">
				<span class="type-small fg:fg-muted" aria-live="polite">
					{#if total > 0 && active !== null}
						{t('compare.position', { current: active + 1, total })}
					{:else if diff.data}
						{t('compare.no_changes')}
					{/if}
				</span>
				<Button
					size="sm"
					variant="secondary"
					disabled={!canStep(active, total, -1) || total === 0}
					onclick={() => step(-1)}
				>
					<ChevronUp size={14} aria-hidden="true" />
					{t('compare.prev')}
				</Button>
				<Button
					size="sm"
					variant="secondary"
					disabled={!canStep(active, total, 1) || total === 0}
					onclick={() => step(1)}
				>
					<ChevronDown size={14} aria-hidden="true" />
					{t('compare.next')}
				</Button>
				<IconButton label={t('compare.close')} onclick={onclose}>
					<X size={18} aria-hidden="true" />
				</IconButton>
			</div>
		</div>
		<div class="flex align-items:center gap:2">
			<Select
				bind:value={from}
				options={points}
				variant="timepoint"
				ariaLabel={t('compare.from')}
				class="w:240px"
			/>
			<IconButton label={t('compare.swap')} onclick={swap}>
				<ArrowLeftRight size={16} aria-hidden="true" />
			</IconButton>
			<Select
				bind:value={to}
				options={points}
				variant="timepoint"
				ariaLabel={t('compare.to')}
				class="w:240px"
			/>
			<span class="flex:1"></span>
			<SegmentedControl
				bind:value={mode}
				ariaLabel={t('compare.mode')}
				options={[
					{ value: 'split', label: t('compare.mode_split') },
					{ value: 'unified', label: t('compare.mode_unified') }
				]}
			/>
		</div>
	</header>

	<DiffView
		diff={diff.data ?? null}
		loading={diff.isPending}
		{mode}
		activeBlock={active}
		leftLabel={labelOf(from)}
		rightLabel={labelOf(to)}
		{onopen}
	/>
</section>
