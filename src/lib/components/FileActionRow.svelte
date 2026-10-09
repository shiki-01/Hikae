<script lang="ts">
	import { Eye, ExternalLink, FolderOpen, Copy, RotateCcw } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { OpenTarget } from '#lib/api/types.js';
	import { OPEN_MENU } from './file-actions';
	import Button from './Button.svelte';
	import SplitButton from './SplitButton.svelte';
	import Tooltip from './Tooltip.svelte';

	interface Props {
		filename: string;
		restoreLabel?: string;
		/** 現在のファイルを開く分割ボタンの表示名 */
		openLabel?: string;
		oncompare?: () => void;
		onrestore?: () => void;
		onopen?: (target: OpenTarget) => void;
		/** 過去の時点の版を読み取り専用で開く（指定すると、現在のファイルを開く分割ボタンの代わりに出す） */
		onopenat?: () => void;
		class?: string;
	}

	let {
		filename,
		restoreLabel = t('file_action.restore_current'),
		openLabel = t('file_action.open'),
		oncompare,
		onrestore,
		onopen,
		onopenat,
		class: className = ''
	}: Props = $props();

	const icons = { default: ExternalLink, folder: FolderOpen, copy_path: Copy } as const;

	const items = $derived(
		OPEN_MENU.map((entry) =>
			entry.type === 'separator'
				? { type: 'separator' as const }
				: {
						label: t(entry.label),
						icon: icons[entry.target],
						onselect: () => onopen?.(entry.target)
					}
		)
	);
</script>

<div
	class={`flex align-items:center justify-content:space-between gap:3 h:48px px:4 flex-shrink:0 bb:1px|solid|border ${className}`}
>
	<h2 class="m:0 min-w:0 type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap">
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
		{#if onopenat}
			<Tooltip text={t('file_action.open_at_hint')} position="bottom-end">
				<Button size="sm" variant="secondary" onclick={onopenat}>
					<ExternalLink size={14} aria-hidden="true" />
					{t('file_action.open_at')}
				</Button>
			</Tooltip>
		{:else if onopen}
			<SplitButton label={openLabel} {items} onclick={() => onopen('default')} />
		{/if}
	</div>
</div>
