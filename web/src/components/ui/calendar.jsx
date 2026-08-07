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
      className={cn("p-3 text-gray-600 dark:text-gray-100", className)}
      classNames={{
        months: "flex flex-col sm:flex-row space-y-4 sm:space-y-0",
        month_caption: "flex justify-center pt-1 pb-3 relative items-center",
        caption_label: "text-sm font-medium",
        nav: "absolute flex items-center justify-between gap-1 inset-x-3 top-3",
        button_previous: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none size-7 bg-transparent p-0 opacity-50 hover:opacity-100 z-50",
        button_next: "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none size-7 bg-transparent p-0 opacity-50 hover:opacity-100 z-50",
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
          "bg-violet-500 text-white hover:bg-violet-500 hover:text-white focus:bg-violet-500 focus:text-white",
        today: "font-bold text-violet-600 dark:text-violet-400",
        outside:
          "day-outside text-gray-400 dark:text-gray-500 aria-selected:bg-violet-500/50 aria-selected:text-gray-400 dark:text-gray-500",
        disabled: "text-gray-400 dark:text-gray-500 opacity-50",
        range_middle:
          "aria-selected:bg-violet-100 dark:aria-selected:bg-violet-400/20 aria-selected:text-gray-700 dark:aria-selected:text-gray-200 aria-selected:rounded-none",
        hidden: "invisible",
        ...classNames,
      }}
      components={{
        Chevron: (props) => {
          if (props.orientation === "left") {
            return <svg className="fill-current" width="7" height="11" viewBox="0 0 7 11"><path d="M5.4 10.8l1.4-1.4-4-4 4-4L5.4 0 0 5.4z" /></svg>;
          }
          return <svg className="fill-current" width="7" height="11" viewBox="0 0 7 11"><path d="M1.4 10.8L0 9.4l4-4-4-4L1.4 0l5.4 5.4z" /></svg>;
        }
      }}
      {...props}
    />
  )
}
Calendar.displayName = "Calendar"

export { Calendar }
