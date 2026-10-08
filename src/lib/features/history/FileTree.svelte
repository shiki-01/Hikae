<script lang="ts">
	import { ChevronRight, File as FileIcon, Folder } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Tooltip from '#lib/components/Tooltip.svelte';
	import type { TreeNode } from './file-tree';

	interface Props {
		nodes: TreeNode[];
		/** 1 ファイルだけを戻す操作を表示するか */
		canRestore: boolean;
		/** この時点の版を読み取り専用で開く */
		onopenat: (path: string) => void;
		onrestore: (path: string) => void;
	}

	let { nodes, canRestore, onopenat, onrestore }: Props = $props();

	const INDENT = 16;
</script>

{#snippet branch(items: TreeNode[], depth: number)}
	<ul class="m:0 p:0 list-style:none">
		{#each items as node (node.path)}
			<li>
				{#if node.kind === 'folder'}
					<details open={depth === 0}>
						<summary
							class="flex align-items:center gap:1 py:1 cursor:pointer type-body"
							style:padding-left={`${depth * INDENT}px`}
						>
							<ChevronRight
								size={16}
								class="chevron fg:fg-muted flex-shrink:0"
								aria-hidden="true"
							/>
							<Folder size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
							<span class="overflow:hidden text-overflow:ellipsis white-space:nowrap"
								>{node.name}</span
							>
						</summary>
						{@render branch(node.children, depth + 1)}
					</details>
				{:else}
					<div
						class="flex align-items:center justify-content:space-between gap:3 py:1"
						style:padding-left={`${depth * INDENT + 20}px`}
					>
						<span class="flex align-items:center gap:2 min-w:0">
							<FileIcon size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
							<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
								>{node.name}</span
							>
						</span>
						<span class="flex gap:2 flex-shrink:0">
							<!-- この時点の版を、読み取り専用の一時ファイルとして開く -->
							<Tooltip text={t('file_action.open_at_hint')} position="bottom-end">
								<Button size="sm" variant="ghost" onclick={() => onopenat(node.path)}>
									{t('file_action.open_at')}
								</Button>
							</Tooltip>
							{#if canRestore}
								<Button size="sm" variant="secondary" onclick={() => onrestore(node.path)}>
									{t('file_action.restore_point')}
								</Button>
							{/if}
						</span>
					</div>
				{/if}
			</li>
		{/each}
	</ul>
{/snippet}

<div class="px:6 py:2" role="group" aria-label={t('history.tree_label')}>
	{@render branch(nodes, 0)}
</div>

<style>
	details[open] > summary > :global(.chevron) {
		transform: rotate(90deg);
	}

	summary {
		list-style: none;
	}

	summary::-webkit-details-marker {
		display: none;
	}
</style>
