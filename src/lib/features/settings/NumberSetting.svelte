<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import Select from '#lib/components/Select.svelte';
	import TextField from '#lib/components/TextField.svelte';
	import { NUMBER_RANGES, parseCustomNumber } from './number-input';
	import { CUSTOM_OPTION, numberSelectValue, type NumberRow } from './settings-model';

	interface Props {
		row: NumberRow;
		/** 現在の設定値 */
		value: number;
		/** 有効な値が確定したとき（選択肢を選んだとき、またはカスタムの入力を確定したとき）に呼ぶ */
		onchange: (value: number) => void;
	}

	let { row, value, onchange }: Props = $props();

	const range = $derived(NUMBER_RANGES[row.key]);

	// 「カスタム」を選んだ直後（まだ値が選択肢に無いとき）も入力欄を出す
	let customMode = $state(false);
	let draft = $state(String(untrack(() => value)));
	let invalid = $state(false);
	let host = $state<HTMLDivElement>();

	const selected = $derived(numberSelectValue(row, value, customMode));
	const showInput = $derived(selected === CUSTOM_OPTION);
	const options = $derived([
		...row.presets.map((preset) => ({ value: String(preset.value), label: t(preset.label) })),
		{ value: CUSTOM_OPTION, label: t('settings.opt_custom') }
	]);

	async function pick(picked: string) {
		invalid = false;
		if (picked === CUSTOM_OPTION) {
			customMode = true;
			draft = String(value);
			await tick();
			host?.querySelector('input')?.focus();
			return;
		}
		customMode = false;
		onchange(Number(picked));
	}

	/** 入力を確定する。範囲外・数値でないときは保存せず、入力欄の下に注記を出す */
	function commit() {
		const parsed = parseCustomNumber(draft, range);
		if (!parsed.ok) {
			invalid = true;
			return;
		}
		invalid = false;
		draft = String(parsed.value);
		if (parsed.value !== value) onchange(parsed.value);
		// 選択肢と同じ値になったら、選択肢の表示に戻す
		if (row.presets.some((preset) => preset.value === parsed.value)) customMode = false;
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			commit();
		}
	}
</script>

<div bind:this={host} class="flex flex-direction:column align-items:flex-end gap:2 flex-shrink:0">
	<Select
		class="w:200px"
		ariaLabel={t(row.label)}
		value={selected}
		{options}
		onchange={(picked) => void pick(picked)}
	/>
	{#if showInput}
		<div class="flex flex-direction:column align-items:flex-end gap:1">
			<div class="flex align-items:center gap:2">
				<TextField
					class="w:96px"
					ariaLabel={`${t(row.label)} (${t('settings.opt_custom')})`}
					inputmode="numeric"
					bind:value={draft}
					oninput={() => (invalid = false)}
					onblur={commit}
					{onkeydown}
				/>
				<span class="type-body fg:fg-muted">{t(row.unit)}</span>
			</div>
			{#if invalid}
				<p class="m:0 type-small fg:state-danger" role="alert">
					{t('settings.custom_range', {
						min: range.min,
						max: range.max,
						unit: t(row.unit)
					})}
				</p>
			{/if}
		</div>
	{/if}
</div>
