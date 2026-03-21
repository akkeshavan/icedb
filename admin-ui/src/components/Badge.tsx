interface BadgeProps {
  value: boolean;
  trueLabel?: string;
  falseLabel?: string;
}

export default function Badge({ value, trueLabel = 'Yes', falseLabel = 'No' }: BadgeProps) {
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${
        value
          ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
          : 'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400'
      }`}
    >
      {value ? trueLabel : falseLabel}
    </span>
  );
}
