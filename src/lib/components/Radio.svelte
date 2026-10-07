<script lang="ts">
	interface Props {
		name: string;
		value: string;
		group: string | null;
		label: string;
		description?: string;
		disabled?: boolean;
		onselect?: (value: string) => void;
		class?: string;
	}

	let {
		name,
		value,
		group,
		label,
		description,
		disabled = false,
		onselect,
		class: className = ''
	}: Props = $props();

	const id = $props.id();
</script>

<div class={`flex align-items:start gap:2 ${className}`}>
	<input
		{id}
		type="radio"
		{name}
		{value}
		{disabled}
		checked={group === value}
		aria-describedby={description ? `${id}-desc` : undefined}
		onchange={() => onselect?.(value)}
		class="size:18px mt:2px accent-color:accent cursor:pointer cursor:not-allowed:disabled opacity:.5:disabled flex-shrink:0"
	/>
	<div class="flex flex-direction:column">
		<label for={id} class="type-body fg:fg cursor:pointer">{label}</label>
		{#if description}
			<span id={`${id}-desc`} class="type-small fg:fg-muted">{description}</span>
		{/if}
	</div>
</div>
