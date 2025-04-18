import { NewSessionHandler } from "@/components/join/newSessionHandler";
import { Suspense } from "react";
import WaitingForSession from "@/components/join/waiting";

export default function Home() {
  return (
    <Suspense fallback={<WaitingForSession />}>
      <NewSessionHandler />
    </Suspense>
  );
}
