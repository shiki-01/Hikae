<script lang="ts">
	import { Minus, Pencil, Plus } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { ImpactItem } from '#lib/api/types.js';

	interface Props {
		items: ImpactItem[];
		timeLabel: string;
		class?: string;
	}

	let { items, timeLabel, class: className = '' }: Props = $props();

	const icons = { modified: Pencil, removed: Minus, restored: Plus };
	const colors = {
		modified: 'fg:fg-muted',
		removed: 'fg:state-danger',
		restored: 'fg:state-saved'
	};
</script>

<ul
	class={`m:0 p:0 list-style:none max-h:320px overflow-y:auto b:1px|solid|border r:md ${className}`}
	aria-label={t('restore.impact_label')}
>
	{#each items as item (item.path)}
		{@const Icon = icons[item.type]}
		<li class="flex align-items:start gap:3 px:3 py:2 bb:1px|solid|border">
			<Icon size={16} class={`${colors[item.type]} flex-shrink:0 mt:3px`} aria-hidden="true" />
			<span class="flex flex-direction:column min-w:0">
				<span class="type-body overflow-wrap:anywhere">{item.path}</span>
				<span class="type-small fg:fg-muted">{t(`restore.${item.type}`, { time: timeLabel })}</span>
			</span>
		</li>
	{/each}
</ul>
