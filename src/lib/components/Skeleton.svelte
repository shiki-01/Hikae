<script lang="ts">
	interface Props {
		class?: string;
	}

	let { class: className = '' }: Props = $props();
</script>

<div aria-hidden="true" class={`motion-pulse bg:bg-subtle r:sm h:16px ${className}`}></div>
