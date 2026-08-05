import type {
  ApplyModelProfileParams,
  ApplyModelProfileResult,
  ModelProfileCandidateList,
  ModelProfileStatus,
} from "@/types/model-profile";

import { invoke, withAddr } from "./transport";

export const modelProfilesClient = {
  status(): Promise<ModelProfileStatus> {
    return invoke<ModelProfileStatus>("service_model_profile_status", withAddr());
  },

  refresh(): Promise<ModelProfileStatus> {
    return invoke<ModelProfileStatus>("service_model_profile_refresh", withAddr());
  },

  candidates(): Promise<ModelProfileCandidateList> {
    return invoke<ModelProfileCandidateList>(
      "service_model_profile_candidates",
      withAddr(),
    );
  },

  apply(input: ApplyModelProfileParams): Promise<ApplyModelProfileResult> {
    return invoke<ApplyModelProfileResult>(
      "service_model_profile_apply",
      withAddr({ payload: input }),
    );
  },
};
