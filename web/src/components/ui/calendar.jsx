"use client"

import * as React from "react"
import { DayPicker } from "react-day-picker"

import { cn } from "../../lib/utils"

function Calendar({
  className,
  classNames,
  showOutsideDays = true,
  ...props
}) {
  return (
    <DayPicker
      showOutsideDays={showOutsideDays}
      className={cn("p-3 text-gray-600 dark:text-gray-100 relative", className)}
      classNames={{
        months: "flex flex-col sm:flex-row space-y-4 sm:space-y-0",
        month_caption: "flex justify-center pt-1 pb-3 relative items-center px-3",
        caption_label: "text-sm font-medium",
        nav: "absolute flex items-center justify-between top-3 left-0 right-0 h-9",
        button_previous: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none size-9 bg-transparent p-0 opacity-60 hover:opacity-100",
        button_next: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none size-9 bg-transparent p-0 opacity-60 hover:opacity-100",
        month_grid: "w-full border-collapse space-y-1",
        weekdays: "flex",
        weekday:
          "text-gray-400 dark:text-gray-500 font-medium rounded-md w-9 text-[0.8rem]",
        week: "flex w-full mt-2",
        // 选中范围使用半透明条，起止用圆点，避免大紫色圆角矩形重叠
        day: "h-9 w-9 text-center text-sm p-0 relative [&:has([aria-selected].day-range-end)]:rounded-r-full [&:has([aria-selected].day-range-start)]:rounded-l-full first:[&:has([aria-selected])]:rounded-l-full last:[&:has([aria-selected])]:rounded-r-full focus-within:relative focus-within:z-20",
        day_button: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 hover:bg-violet-100 dark:hover:bg-violet-400/20 h-9 w-9 p-0",
        range_start: "rounded-l-full",
        range_end: "day-range-end rounded-r-full",
        selected:
          "bg-violet-500 text-white hover:bg-violet-500 hover:text-white focus:bg-violet-500 focus:text-white aria-selected:bg-violet-500 aria-selected:hover:bg-violet-500",
        today: "font-bold text-violet-600 dark:text-violet-400",
        outside:
          "day-outside text-gray-400 dark:text-gray-500 aria-selected:bg-violet-500/50 aria-selected:text-gray-400 dark:text-gray-500",
        disabled: "text-gray-400 dark:text-gray-500 opacity-50",
        range_middle:
          "aria-selected:bg-violet-100 dark:aria-selected:bg-violet-400/20 aria-selected:text-gray-700 dark:aria-selected:text-gray-200 aria-selected:rounded-none aria-selected:hover:bg-violet-100 dark:aria-selected:hover:bg-violet-400/20",
        hidden: "invisible",
        ...classNames,
      }}
      components={{
        Chevron: (props) => {
          if (props.orientation === "left") {
            return <svg className="fill-current" width="10" height="16" viewBox="0 0 10 16"><path d="M8.4 16 0 8l8.4-8L10 1.4 2.8 8l7.2 6.6L8.4 16Z" /></svg>;
          }
          return <svg className="fill-current" width="10" height="16" viewBox="0 0 10 16"><path d="M1.6 16 0 14.6 7.2 8 0 1.4 1.6 0 10 8l-8.4 8Z" /></svg>;
        }
      }}
      {...props}
    />
  )
}
Calendar.displayName = "Calendar"

export { Calendar }
