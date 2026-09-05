import { Navigate, createFileRoute, redirect } from "@tanstack/react-router";
import type { ReactElement } from "react";

/** 应用默认打开看板，根路径不再停留在中性首页。 */
export const Route = createFileRoute("/")({
  beforeLoad: () => {
    throw redirect({ replace: true, to: "/dashboard" });
  },
  component: DefaultLandingRedirect,
});

/** 在 beforeLoad 未改写时仍把根路径替换到看板。 */
function DefaultLandingRedirect(): ReactElement {
  return <Navigate replace to="/dashboard" />;
}
