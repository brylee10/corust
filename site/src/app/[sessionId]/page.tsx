import UserJoin from "@/components/join/userJoin";
import WaitingForSession from "@/components/join/waiting";
import { Suspense } from "react";

export default function Session() {
  return (
    <Suspense fallback={<WaitingForSession />}>
      <UserJoin />
    </Suspense>
  );
}
