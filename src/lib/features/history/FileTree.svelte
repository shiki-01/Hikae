<script lang="ts">
	import { ChevronRight, File as FileIcon, Folder } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import { formatBytes } from '#lib/i18n/format.js';
	import Button from '#lib/components/Button.svelte';
	import ChangeBadge from '#lib/components/ChangeBadge.svelte';
	import Tooltip from '#lib/components/Tooltip.svelte';
	import { DROP_FOLDER_ATTR } from '#lib/features/changes/drop-target.js';
	import type { TreeFolder, TreeNode } from './file-tree';

	interface Props {
		nodes: TreeNode[];
		/** 1 ファイルだけを戻す操作を表示するか（`onrestore` と合わせて指定する） */
		canRestore?: boolean;
		/** この時点の版を読み取り専用で開く。指定したときだけ、各ファイルに操作を出す */
		onopenat?: (path: string) => void;
		onrestore?: (path: string) => void;
		/** 指定すると、ファイルの行を選択できる行にする（プロジェクトフォルダの一覧用） */
		onselect?: (path: string) => void;
		selectedPath?: string | null;
		/** フォルダを最初から開いておくか。既定は最上位のフォルダだけ */
		defaultOpen?: (folder: TreeFolder, depth: number) => boolean;
		/** ツリー全体の読み上げ名 */
		label?: string;
		/** 各ファイルの行に、サイズを添えるか */
		showSize?: boolean;
		/** フォルダの行を、ファイルのドロップ先にするか（行に追加先の印を付ける） */
		dropTargets?: boolean;
		/** ドラッグ中の位置にあるフォルダ（強調する） */
		activeDropFolder?: string | null;
	}

	let {
		nodes,
		canRestore = false,
		onopenat,
		onrestore,
		onselect,
		selectedPath = null,
		defaultOpen = (_folder, depth) => depth === 0,
		label = t('history.tree_label'),
		showSize = false,
		dropTargets = false,
		activeDropFolder = null
	}: Props = $props();

	const INDENT = 16;

	// 利用者が開閉したフォルダ。操作していないフォルダは `defaultOpen` に従う。
	// 閉じているフォルダの中身は描画しない（ファイルが多いときに重くならないように）
	let toggled = $state<Record<string, boolean>>({});

	function isOpen(folder: TreeFolder, depth: number): boolean {
		return toggled[folder.path] ?? defaultOpen(folder, depth);
	}

	function toggle(folder: TreeFolder, depth: number) {
		toggled[folder.path] = !isOpen(folder, depth);
	}
</script>

{#snippet branch(items: TreeNode[], depth: number)}
	<ul class="m:0 p:0 list-style:none">
		{#each items as node (node.path)}
			<li>
				{#if node.kind === 'folder'}
					{@const open = isOpen(node, depth)}
					<details {open}>
						<summary
							class={`flex align-items:center gap:1 py:1 cursor:pointer type-body ${
								dropTargets && activeDropFolder === node.path ? 'bg:accent-subtle' : ''
							}`}
							style:padding-left={`${depth * INDENT}px`}
							{...dropTargets ? { [DROP_FOLDER_ATTR]: node.path } : {}}
							onclick={(event) => {
								event.preventDefault();
								toggle(node, depth);
							}}
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
						{#if open}
							{@render branch(node.children, depth + 1)}
						{/if}
					</details>
				{:else}
					{@const selected = selectedPath === node.path}
					<div
						class={`flex align-items:center justify-content:space-between gap:3 py:1 ${
							selected ? 'bg:accent-subtle' : ''
						}`}
						style:padding-left={`${depth * INDENT + 20}px`}
					>
						{#if onselect}
							<button
								type="button"
								aria-current={selected ? 'true' : undefined}
								onclick={() => onselect(node.path)}
								class="flex align-items:center gap:2 flex:1 min-w:0 b:0 p:0 bg:transparent text-align:left cursor:pointer fg:fg"
							>
								<FileIcon size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
								<span
									class="flex:1 min-w:0 type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
									>{node.name}</span
								>
								{#if showSize && node.size !== null}
									<span class="type-small fg:fg-muted flex-shrink:0">{formatBytes(node.size)}</span>
								{/if}
							</button>
							{#if node.change}
								<ChangeBadge type={node.change} conflict={node.isConflict === true} class="pr:3" />
							{:else if node.isConflict}
								<ChangeBadge type="modified" conflict class="pr:3" />
							{/if}
						{:else}
							<span class="flex align-items:center gap:2 min-w:0">
								<FileIcon size={16} class="fg:fg-muted flex-shrink:0" aria-hidden="true" />
								<span class="type-body overflow:hidden text-overflow:ellipsis white-space:nowrap"
									>{node.name}</span
								>
							</span>
						{/if}
						{#if onopenat || (canRestore && onrestore)}
							<span class="flex gap:2 flex-shrink:0">
								<!-- この時点の版を、読み取り専用の一時ファイルとして開く -->
								{#if onopenat}
									<Tooltip text={t('file_action.open_at_hint')} position="bottom-end">
										<Button size="sm" variant="ghost" onclick={() => onopenat(node.path)}>
											{t('file_action.open_at')}
										</Button>
									</Tooltip>
								{/if}
								{#if canRestore && onrestore}
									<Button size="sm" variant="secondary" onclick={() => onrestore(node.path)}>
										{t('file_action.restore_point')}
									</Button>
								{/if}
							</span>
						{/if}
					</div>
				{/if}
			</li>
		{/each}
	</ul>
{/snippet}

<div class={onselect ? 'py:2' : 'px:6 py:2'} role="group" aria-label={label}>
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
