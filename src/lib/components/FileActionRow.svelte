<script lang="ts">
	import { Eye, ExternalLink, FolderOpen, Copy, RotateCcw, Code } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { OpenTarget } from '#lib/api/types.js';
	import Button from './Button.svelte';
	import SplitButton from './SplitButton.svelte';

	interface Props {
		filename: string;
		restoreLabel?: string;
		oncompare?: () => void;
		onrestore?: () => void;
		onopen?: (target: OpenTarget) => void;
		class?: string;
	}

	let {
		filename,
		restoreLabel = t('file_action.restore_current'),
		oncompare,
		onrestore,
		onopen,
		class: className = ''
	}: Props = $props();

	const items = $derived([
		{
			label: t('file_action.open_default'),
			icon: ExternalLink,
			onselect: () => onopen?.('default')
		},
		{ label: t('file_action.open_vscode'), icon: Code, onselect: () => onopen?.('vscode') },
		{ label: t('file_action.show_folder'), icon: FolderOpen, onselect: () => onopen?.('folder') },
		{ type: 'separator' as const },
		{ label: t('file_action.copy_path'), icon: Copy, onselect: () => onopen?.('copy_path') }
	]);
</script>

<div
	class={`flex align-items:center justify-content:space-between gap:3 h:48px px:4 flex-shrink:0 bb:1px|solid|border ${className}`}
>
	<h2 class="m:0 type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap">
		{filename}
	</h2>
	<div class="flex align-items:center gap:2 flex-shrink:0">
		{#if oncompare}
			<Button size="sm" variant="secondary" onclick={oncompare}>
				<Eye size={14} aria-hidden="true" />
				{t('file_action.compare')}
			</Button>
		{/if}
		{#if onrestore}
			<Button size="sm" variant="secondary" onclick={onrestore}>
				<RotateCcw size={14} aria-hidden="true" />
				{restoreLabel}
			</Button>
		{/if}
		{#if onopen}
			<SplitButton label={t('file_action.open')} {items} onclick={() => onopen('default')} />
		{/if}
	</div>
</div>
