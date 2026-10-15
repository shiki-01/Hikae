<script lang="ts">
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import type { Snippet } from 'svelte';
	import { t } from '#lib/i18n/index.js';

	interface Props {
		title: string;
		subtitle?: string | null;
		onback?: () => void;
		actions?: Snippet;
	}

	let { title, subtitle = null, onback, actions }: Props = $props();
</script>

<header
	class="flex align-items:center gap:2 px:2 py:2 min-h:56px bb:1px|solid|border flex-shrink:0"
>
	{#if onback}
		<button
			type="button"
			aria-label={t('mobile.common.back')}
			onclick={onback}
			class="inline-flex align-items:center justify-content:center min-w:44px min-h:44px b:0 r:md bg:transparent fg:fg cursor:pointer bg:bg-subtle:hover flex-shrink:0"
		>
			<ArrowLeft size={22} aria-hidden="true" />
		</button>
	{/if}
	<div class="flex flex-direction:column flex:1 min-w:0 px:2">
		<h1 class="m:0 type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap">
			{title}
		</h1>
		{#if subtitle}
			<span class="type-small fg:fg-muted overflow:hidden text-overflow:ellipsis white-space:nowrap"
				>{subtitle}</span
			>
		{/if}
	</div>
	{#if actions}
		<div class="flex align-items:center gap:1 flex-shrink:0">
			{@render actions()}
		</div>
	{/if}
</header>
