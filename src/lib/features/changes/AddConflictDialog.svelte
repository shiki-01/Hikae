<script lang="ts">
	import { ShieldCheck } from '@lucide/svelte';
	import { t } from '#lib/i18n/index.js';
	import type { AddConflictAction, AddConflictDecision, NameConflict } from '#lib/api/types.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import Radio from '#lib/components/Radio.svelte';
	import Select, { type SelectOption } from '#lib/components/Select.svelte';
	import {
		applyToAll,
		choose,
		commonChoice,
		defaultChoices,
		splitPath,
		toDecisions,
		type ConflictChoices
	} from './add-conflict';

	interface Props {
		/** 確認が必要な同名のファイル。null のときは閉じている */
		conflicts: NameConflict[] | null;
		/** 選択後に追加をやり直している間は true */
		pending: boolean;
		onconfirm: (decisions: AddConflictDecision[]) => void;
		oncancel: () => void;
	}

	let { conflicts, pending, onconfirm, oncancel }: Props = $props();

	// 新しい確認の一覧が来たら、選択を「両方残す」にそろえる（選んだ内容は、この値に上書きしていく）
	let choices = $derived<ConflictChoices>(conflicts ? defaultChoices(conflicts) : {});

	const list = $derived(conflicts ?? []);
	const common = $derived(commonChoice(list, choices));
	const allOptions: SelectOption[] = [
		{ value: 'keep_both', label: t('add_conflict.keep_both') },
		{ value: 'replace', label: t('add_conflict.replace') },
		{ value: 'skip', label: t('add_conflict.skip') }
	];

	function setAll(value: string) {
		if (value === 'keep_both' || value === 'replace' || value === 'skip') {
			choices = applyToAll(list, value);
		}
	}

	function setOne(path: string, value: string) {
		if (value === 'keep_both' || value === 'replace' || value === 'skip') {
			choices = choose(choices, list, path, value satisfies AddConflictAction);
		}
	}
</script>

<Dialog
	open={conflicts !== null}
	variant="warning"
	title={t('add_conflict.title')}
	description={t('add_conflict.description')}
	busy={pending}
	onclose={oncancel}
>
	{#if conflicts}
		<div class="flex flex-direction:column gap:3">
			{#if conflicts.length > 1}
				<div class="flex align-items:center justify-content:space-between gap:3">
					<span class="type-body font-weight:700">{t('add_conflict.apply_all')}</span>
					<Select
						options={allOptions}
						value={common ?? ''}
						ariaLabel={t('add_conflict.apply_all_label')}
						placeholder={t('add_conflict.apply_all')}
						disabled={pending}
						onchange={setAll}
					/>
				</div>
			{/if}
			<ul
				class="m:0 p:0 list-style:none b:1px|solid|border r:md"
				aria-label={t('add_conflict.list_label')}
			>
				{#each conflicts as conflict, index (conflict.path)}
					{@const parts = splitPath(conflict.path)}
					<li class="flex flex-direction:column gap:2 px:3 py:3 bb:1px|solid|border">
						<span class="type-body font-weight:700 overflow-wrap:anywhere">{parts.name}</span>
						{#if parts.folder}
							<span class="type-small fg:fg-muted overflow-wrap:anywhere">{parts.folder}</span>
						{/if}
						<div
							role="radiogroup"
							aria-label={t('add_conflict.file_choice', { path: conflict.path })}
							class="flex flex-direction:column gap:2"
						>
							<Radio
								name={`add-conflict-${index}`}
								value="keep_both"
								group={choices[conflict.path] ?? 'keep_both'}
								label={t('add_conflict.keep_both')}
								description={t('add_conflict.keep_both_hint')}
								disabled={pending}
								onselect={(value) => setOne(conflict.path, value)}
							/>
							<Radio
								name={`add-conflict-${index}`}
								value="replace"
								group={choices[conflict.path] ?? 'keep_both'}
								label={t('add_conflict.replace')}
								description={conflict.canReplace
									? undefined
									: t('add_conflict.replace_unavailable')}
								disabled={pending || !conflict.canReplace}
								onselect={(value) => setOne(conflict.path, value)}
							/>
							<Radio
								name={`add-conflict-${index}`}
								value="skip"
								group={choices[conflict.path] ?? 'keep_both'}
								label={t('add_conflict.skip')}
								disabled={pending}
								onselect={(value) => setOne(conflict.path, value)}
							/>
						</div>
					</li>
				{/each}
			</ul>
		</div>
	{/if}

	{#snippet footerNote()}
		<div class="flex align-items:start gap:2 p:3 r:md bg:bg-subtle">
			<ShieldCheck size={18} class="fg:state-saved flex-shrink:0 mt:2px" aria-hidden="true" />
			<p class="m:0 type-body">{t('add_conflict.safe')}</p>
		</div>
	{/snippet}

	{#snippet actions()}
		<Button variant="secondary" disabled={pending} onclick={oncancel}>
			{t('add_conflict.cancel')}
		</Button>
		<Button
			variant="primary"
			loading={pending}
			disabled={pending}
			onclick={() => conflicts && onconfirm(toDecisions(conflicts, choices))}
		>
			{t('add_conflict.confirm')}
		</Button>
	{/snippet}
</Dialog>
