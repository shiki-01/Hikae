<script lang="ts">
	interface Props {
		checked?: boolean;
		label: string;
		description?: string;
		disabled?: boolean;
		onchange?: (checked: boolean) => void;
		class?: string;
	}

	let {
		checked = $bindable(false),
		label,
		description,
		disabled = false,
		onchange,
		class: className = ''
	}: Props = $props();

	const id = $props.id();

	function handleChange(event: Event) {
		checked = (event.currentTarget as HTMLInputElement).checked;
		onchange?.(checked);
	}
</script>

<div class={`flex align-items:start gap:2 ${className}`}>
	<input
		{id}
		type="checkbox"
		{checked}
		{disabled}
		aria-describedby={description ? `${id}-desc` : undefined}
		onchange={handleChange}
		class="size:18px mt:2px accent-color:accent cursor:pointer cursor:not-allowed:disabled opacity:.5:disabled flex-shrink:0"
	/>
	<div class="flex flex-direction:column">
		<label for={id} class={`type-body cursor:pointer ${disabled ? 'fg:fg-faint' : 'fg:fg'}`}
			>{label}</label
		>
		{#if description}
			<span id={`${id}-desc`} class="type-small fg:fg-muted">{description}</span>
		{/if}
	</div>
</div>
