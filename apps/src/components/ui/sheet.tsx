"use client";

import * as React from "react";
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog";
import { XIcon } from "lucide-react";
import { buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";

function Sheet({ ...props }: DialogPrimitive.Root.Props) {
  return <DialogPrimitive.Root data-slot="sheet" {...props} />;
}

function SheetContent({
  className,
  children,
  ...props
}: DialogPrimitive.Popup.Props) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Backdrop
        className="fixed inset-0 z-50 bg-black/35 supports-backdrop-filter:backdrop-blur-sm data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
      />
      <DialogPrimitive.Viewport className="fixed inset-0 z-50 flex justify-end outline-none">
        <DialogPrimitive.Popup
          data-slot="sheet-content"
          className={cn(
            "glass-card mission-panel relative flex h-dvh w-full max-w-[min(100vw,560px)] flex-col overflow-hidden rounded-none border-l bg-background p-5 shadow-2xl outline-none data-open:animate-in data-open:slide-in-from-right data-closed:animate-out data-closed:slide-out-to-right",
            className,
          )}
          {...props}
        >
          {children}
          <DialogPrimitive.Close
            className={cn(buttonVariants({ variant: "ghost", size: "icon-sm" }), "absolute top-3 right-3")}
            type="button"
          >
            <XIcon />
            <span className="sr-only">Close</span>
          </DialogPrimitive.Close>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Viewport>
    </DialogPrimitive.Portal>
  );
}

function SheetHeader({ className, ...props }: React.ComponentProps<"div">) {
  return <div className={cn("shrink-0 pr-9", className)} {...props} />;
}

function SheetTitle({ className, ...props }: DialogPrimitive.Title.Props) {
  return <DialogPrimitive.Title className={cn("text-base font-medium", className)} {...props} />;
}

export { Sheet, SheetContent, SheetHeader, SheetTitle };
