//! Shared, bounded frame-route decisions used by export and profiling.

use crate::options::{EncodeBackendPreference, TransferSyntax};
use crate::Error;

/// A route selected from source-frame capabilities and immutable execution policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlannedFrameRoute {
    JpegPassthrough,
    JpegRetile,
    JpegCpuEncode,
    JpegDeviceEncodeCandidate,
    J2kPassthrough,
    DirectJpegToHtj2k,
    DirectJ2kToHtj2k,
    J2kCpuEncode,
    J2kDeviceEncodeCandidate,
    Blank,
    Unsupported(RouteRejection),
}

/// Stable reasons why a frame-route candidate was not selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteRejection {
    SourceRouteUnavailable,
    TransferSyntaxMismatch,
    PassthroughRequired,
    DirectTranscodeRequired,
}

/// Candidate rejection details retained without allocating per frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RouteRejections {
    pub(crate) passthrough: Option<RouteRejection>,
    pub(crate) retile: Option<RouteRejection>,
    pub(crate) direct_j2k: Option<RouteRejection>,
    pub(crate) direct_jpeg: Option<RouteRejection>,
}

/// The complete policy result for one bounded-plan frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRouteDecision {
    pub(crate) route: PlannedFrameRoute,
    pub(crate) fallback: Option<PlannedFrameRoute>,
    pub(crate) transfer_syntax: TransferSyntax,
    pub(crate) rejections: RouteRejections,
}

impl FrameRouteDecision {
    pub(crate) fn allows_j2k_encode_fallback(self) -> bool {
        matches!(
            self.route,
            PlannedFrameRoute::J2kCpuEncode | PlannedFrameRoute::J2kDeviceEncodeCandidate
        ) || matches!(
            self.fallback,
            Some(PlannedFrameRoute::J2kCpuEncode | PlannedFrameRoute::J2kDeviceEncodeCandidate)
        )
    }
}

/// Source capabilities established by codec-specific frame inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameRouteSource {
    Jpeg {
        passthrough: bool,
        retile: bool,
        blank: bool,
    },
    J2k {
        passthrough: bool,
        direct_j2k: bool,
        direct_jpeg: bool,
        j2k_reencode: bool,
    },
}

/// Immutable route policy needed before backend measurement or execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouteExecutionContext {
    transfer_syntax: TransferSyntax,
    encode_backend: EncodeBackendPreference,
}

impl RouteExecutionContext {
    pub(crate) const fn new(
        transfer_syntax: TransferSyntax,
        encode_backend: EncodeBackendPreference,
    ) -> Self {
        Self {
            transfer_syntax,
            encode_backend,
        }
    }
}

/// Authoritative route-policy evaluator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoutePlanner {
    context: RouteExecutionContext,
}

impl RoutePlanner {
    pub(crate) const fn new(context: RouteExecutionContext) -> Self {
        Self { context }
    }

    pub(crate) fn decide(self, source: FrameRouteSource) -> FrameRouteDecision {
        match source {
            FrameRouteSource::Jpeg {
                passthrough,
                retile,
                blank,
            } => self.decide_jpeg(passthrough, retile, blank),
            FrameRouteSource::J2k {
                passthrough,
                direct_j2k,
                direct_jpeg,
                j2k_reencode,
            } => self.decide_j2k(passthrough, direct_j2k, direct_jpeg, j2k_reencode),
        }
    }

    fn decide_jpeg(self, passthrough: bool, retile: bool, blank: bool) -> FrameRouteDecision {
        if self.context.transfer_syntax != TransferSyntax::JpegBaseline8Bit {
            return self.unsupported(RouteRejection::TransferSyntaxMismatch);
        }
        let mut rejections = RouteRejections::default();
        if passthrough {
            return self.decision(PlannedFrameRoute::JpegPassthrough, rejections);
        }
        rejections.passthrough = Some(RouteRejection::SourceRouteUnavailable);
        if retile {
            return self.decision(PlannedFrameRoute::JpegRetile, rejections);
        }
        rejections.retile = Some(RouteRejection::SourceRouteUnavailable);
        if blank {
            return self.decision(PlannedFrameRoute::Blank, rejections);
        }
        let route = if self.context.encode_backend == EncodeBackendPreference::CpuOnly {
            PlannedFrameRoute::JpegCpuEncode
        } else {
            PlannedFrameRoute::JpegDeviceEncodeCandidate
        };
        self.decision(route, rejections)
    }

    fn decide_j2k(
        self,
        passthrough: bool,
        direct_j2k: bool,
        direct_jpeg: bool,
        j2k_reencode: bool,
    ) -> FrameRouteDecision {
        if !self.context.transfer_syntax.is_j2k_family() {
            return self.unsupported(RouteRejection::TransferSyntaxMismatch);
        }
        let mut rejections = RouteRejections::default();
        if passthrough {
            return self.decision(PlannedFrameRoute::J2kPassthrough, rejections);
        }
        rejections.passthrough = Some(RouteRejection::SourceRouteUnavailable);
        if direct_j2k {
            return self.direct_j2k_decision(PlannedFrameRoute::DirectJ2kToHtj2k, rejections);
        }
        rejections.direct_j2k = Some(RouteRejection::SourceRouteUnavailable);
        if direct_jpeg {
            return self.direct_j2k_decision(PlannedFrameRoute::DirectJpegToHtj2k, rejections);
        }
        rejections.direct_jpeg = Some(RouteRejection::SourceRouteUnavailable);

        if self.context.transfer_syntax == TransferSyntax::Htj2k {
            return self.decision(
                PlannedFrameRoute::Unsupported(RouteRejection::DirectTranscodeRequired),
                rejections,
            );
        }
        if self.context.transfer_syntax.is_jpeg2000_passthrough_only() {
            if j2k_reencode {
                return self.decision(PlannedFrameRoute::J2kCpuEncode, rejections);
            }
            return self.decision(
                PlannedFrameRoute::Unsupported(RouteRejection::PassthroughRequired),
                rejections,
            );
        }
        let route = if self.context.encode_backend == EncodeBackendPreference::CpuOnly {
            PlannedFrameRoute::J2kCpuEncode
        } else {
            PlannedFrameRoute::J2kDeviceEncodeCandidate
        };
        self.decision(route, rejections)
    }

    fn unsupported(self, reason: RouteRejection) -> FrameRouteDecision {
        self.decision(
            PlannedFrameRoute::Unsupported(reason),
            RouteRejections::default(),
        )
    }

    fn decision(self, route: PlannedFrameRoute, rejections: RouteRejections) -> FrameRouteDecision {
        FrameRouteDecision {
            route,
            fallback: None,
            transfer_syntax: self.context.transfer_syntax,
            rejections,
        }
    }

    fn direct_j2k_decision(
        self,
        route: PlannedFrameRoute,
        rejections: RouteRejections,
    ) -> FrameRouteDecision {
        let fallback = matches!(
            self.context.transfer_syntax,
            TransferSyntax::Jpeg2000Lossless
                | TransferSyntax::Htj2kLossless
                | TransferSyntax::Htj2kLosslessRpcl
        )
        .then_some(
            if self.context.encode_backend == EncodeBackendPreference::CpuOnly {
                PlannedFrameRoute::J2kCpuEncode
            } else {
                PlannedFrameRoute::J2kDeviceEncodeCandidate
            },
        );
        FrameRouteDecision {
            route,
            fallback,
            transfer_syntax: self.context.transfer_syntax,
            rejections,
        }
    }
}

pub(crate) fn incompatible_frame_route(codec: &str, route: PlannedFrameRoute) -> Error {
    Error::Unsupported {
        reason: format!(
            "{codec} route planner produced {route:?} for an incompatible planned frame"
        ),
    }
}
