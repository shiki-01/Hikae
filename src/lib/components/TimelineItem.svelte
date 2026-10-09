<script lang="ts">
	import { ChevronDown, ChevronRight, Cloud } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatDateTime } from '#lib/i18n/format.js';
	import type { TimelineEntry } from './timeline';

	interface Props {
		entry: TimelineEntry;
		selectedId?: string | null;
		expanded?: boolean;
		isLast?: boolean;
		onselect?: (id: string) => void;
		ontoggle?: (id: string) => void;
	}

	let {
		entry,
		selectedId = null,
		expanded = false,
		isLast = false,
		onselect,
		ontoggle
	}: Props = $props();

	const rail = $derived(isLast ? '' : 'bg:border-strong');
	const selected = (id: string) => selectedId === id;
	const rowLook = (id: string) =>
		selected(id) ? 'bg:accent-subtle' : 'bg:transparent bg:bg-subtle:hover';
</script>

{#if entry.kind === 'cloud_line'}
	<li class="flex align-items:center gap:3 py:1" aria-label={t('timeline.cloud_line')}>
		<span class="w:24px flex justify-content:center flex-shrink:0">
			<Cloud size={16} class="fg:state-sync" aria-hidden="true" />
		</span>
		<span class="type-small fg:fg-muted font-weight:500">{t('timeline.cloud_line')}</span>
		<span class="flex:1 h:1px bg:state-sync"></span>
	</li>
{:else}
	<li class="flex gap:3">
		<div class="w:24px flex flex-direction:column align-items:center flex-shrink:0">
			{#if entry.kind === 'now'}
				<span
					class="mt:3 size:14px r:full b:2px|solid|accent flex align-items:center justify-content:center"
					aria-hidden="true"
				>
					<span class="size:6px r:full bg:accent"></span>
				</span>
			{:else if entry.kind === 'save'}
				<span class="mt:3 size:12px r:full bg:accent" aria-hidden="true"></span>
			{:else}
				<span class="mt:3 size:8px r:full bg:snapshot" aria-hidden="true"></span>
			{/if}
			<span class={`flex:1 w:2px min-h:8px ${rail}`}></span>
		</div>

		<div class="flex:1 min-w:0 pb:2">
			{#if entry.kind === 'now'}
				<button
					type="button"
					aria-current={selected('now') ? 'true' : undefined}
					onclick={() => onselect?.('now')}
					class={`w:100% px:3 py:2 r:md b:0 text-align:left cursor:pointer fg:fg ${rowLook('now')}`}
				>
					<span class="type-body font-weight:700">{t('timeline.now')}</span>
				</button>
			{:else if entry.kind === 'save'}
				<button
					type="button"
					aria-current={selected(entry.id) ? 'true' : undefined}
					onclick={() => onselect?.(entry.id)}
					class={`flex flex-direction:column w:100% px:3 py:2 r:md b:0 text-align:left cursor:pointer fg:fg ${rowLook(entry.id)}`}
				>
					<span class="type-small fg:fg-muted overflow-wrap:anywhere">
						{formatDateTime(entry.point.createdAt)}
						{#if entry.point.pcName}
							・{entry.point.pcName}
						{/if}
					</span>
					<span class="type-body overflow-wrap:anywhere">{entry.point.message}</span>
				</button>
			{:else}
				<button
					type="button"
					aria-expanded={expanded}
					onclick={() => ontoggle?.(entry.id)}
					class="flex align-items:center gap:1 px:3 py:1 r:md b:0 bg:transparent fg:fg-muted type-small cursor:pointer bg:bg-subtle:hover"
				>
					{#if expanded}
						<ChevronDown size={14} aria-hidden="true" />
					{:else}
						<ChevronRight size={14} aria-hidden="true" />
					{/if}
					{t('timeline.autos', { count: entry.points.length })}
				</button>
				{#if expanded}
					<ul class="m:0 p:0 list-style:none">
						{#each entry.points as point (point.id)}
							<li>
								<button
									type="button"
									aria-current={selected(point.id) ? 'true' : undefined}
									onclick={() => onselect?.(point.id)}
									class={`w:100% pl:6 pr:3 py:1 overflow-wrap:anywhere r:md b:0 text-align:left cursor:pointer fg:fg-muted type-small ${rowLook(point.id)}`}
								>
									{formatDateTime(point.createdAt)}
									{t('timeline.auto_save')}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			{/if}
		</div>
	</li>
{/if}
