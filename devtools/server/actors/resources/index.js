/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

"use strict";

const Targets = require("devtools/server/actors/targets/index");

const TYPES = {
  CONSOLE_MESSAGE: "console-message",
  CSS_CHANGE: "css-change",
  CSS_MESSAGE: "css-message",
  DOCUMENT_EVENT: "document-event",
  ERROR_MESSAGE: "error-message",
  PLATFORM_MESSAGE: "platform-message",
  NETWORK_EVENT: "network-event",
  STYLESHEET: "stylesheet",
};
exports.TYPES = TYPES;

// Helper dictionaries, which will contain data specific to each resource type.
// - `path` is the absolute path to the module defining the Resource Watcher class,
// Also see the attributes added by `augmentResourceDictionary` for each type:
// - `watchers` is a weak map which will store Resource Watchers
//    (i.e. devtools/server/actors/resources/ class instances)
//    keyed by target actor -or- watcher actor.
// - `WatcherClass` is a shortcut to the Resource Watcher module.
//    Each module exports a Resource Watcher class.
// These lists are specific for the parent process and each target type.
const FrameTargetResources = augmentResourceDictionary({
  [TYPES.CONSOLE_MESSAGE]: {
    path: "devtools/server/actors/resources/console-messages",
  },
  [TYPES.CSS_CHANGE]: {
    path: "devtools/server/actors/resources/css-changes",
  },
  [TYPES.CSS_MESSAGE]: {
    path: "devtools/server/actors/resources/css-messages",
  },
  [TYPES.DOCUMENT_EVENT]: {
    path: "devtools/server/actors/resources/document-event",
  },
  [TYPES.ERROR_MESSAGE]: {
    path: "devtools/server/actors/resources/error-messages",
  },
  [TYPES.PLATFORM_MESSAGE]: {
    path: "devtools/server/actors/resources/platform-messages",
  },
  [TYPES.STYLESHEET]: {
    path: "devtools/server/actors/resources/stylesheets",
  },
});
const ParentProcessResources = augmentResourceDictionary({
  [TYPES.NETWORK_EVENT]: {
    path: "devtools/server/actors/resources/network-events",
  },
});

function augmentResourceDictionary(dict) {
  for (const resource of Object.values(dict)) {
    resource.watchers = new WeakMap();

    loader.lazyRequireGetter(resource, "WatcherClass", resource.path);
  }
  return dict;
}

/**
 * For a given actor, return the related dictionary defined just before,
 * that contains info about how to listen for a given resource type, from a given actor.
 *
 * @param Actor watcherOrTargetActor
 *        Either a WatcherActor or a TargetActor which can be listening to a resource.
 */
function getResourceTypeDictionary(watcherOrTargetActor) {
  const { typeName } = watcherOrTargetActor;
  if (typeName == "watcher") {
    return ParentProcessResources;
  }
  const { targetType } = watcherOrTargetActor;
  return getResourceTypeDictionaryForTargetType(targetType);
}

/**
 * For a targetType, return the related dictionary.
 *
 * @param String targetType
 *        A targetType string (See Targets.TYPES)
 */
function getResourceTypeDictionaryForTargetType(targetType) {
  switch (targetType) {
    case Targets.TYPES.FRAME:
      return FrameTargetResources;
    default:
      throw new Error(`Unsupported target actor typeName '${targetType}'`);
  }
}

/**
 * For a given actor, return the object stored in one of the previous dictionary
 * that contains info about how to listen for a given resource type, from a given actor.
 *
 * @param Actor watcherOrTargetActor
 *        Either a WatcherActor or a TargetActor which can be listening to a resource.
 * @param String resourceType
 *        The resource type to be observed.
 */
function getResourceTypeEntry(watcherOrTargetActor, resourceType) {
  const dict = getResourceTypeDictionary(watcherOrTargetActor);
  if (!(resourceType in dict)) {
    throw new Error(
      `Unsupported resource type '${resourceType}' for ${watcherOrTargetActor.typeName}`
    );
  }
  return dict[resourceType];
}

/**
 * Start watching for a new list of resource types.
 * This will also emit all already existing resources before resolving.
 *
 * @param Actor watcherOrTargetActor
 *        Either a WatcherActor or a TargetActor which can be listening to a resource.
 *        WatcherActor will be used for resources listened from the parent process,
 *        and TargetActor will be used for resources listened from the content process.
 *        This actor:
 *          - defines what context to observe (browsing context, process, worker, ...)
 *            Via browsingContextID, windows, docShells attributes for the target actor.
 *            Via browserId or browserElement attributes for the watcher actor.
 *          - exposes `notifyResourceAvailable` method to be notified about the available resources
 * @param Array<String> resourceTypes
 *        List of all type of resource to listen to.
 */
async function watchResources(watcherOrTargetActor, resourceTypes) {
  // If we are given a target actor, filter out the resource types supported by the target.
  // When using sharedData to pass types between processes, we are passing them for all target types.
  const { targetType } = watcherOrTargetActor;
  if (targetType) {
    resourceTypes = getResourceTypesForTargetType(resourceTypes, targetType);
  }
  for (const resourceType of resourceTypes) {
    const { watchers, WatcherClass } = getResourceTypeEntry(
      watcherOrTargetActor,
      resourceType
    );

    // Ignore resources we're already listening to
    if (watchers.has(watcherOrTargetActor)) {
      continue;
    }

    const watcher = new WatcherClass();
    await watcher.watch(watcherOrTargetActor, {
      onAvailable: watcherOrTargetActor.notifyResourceAvailable,
      onDestroyed: watcherOrTargetActor.notifyResourceDestroyed,
      onUpdated: watcherOrTargetActor.notifyResourceUpdated,
    });
    watchers.set(watcherOrTargetActor, watcher);
  }
}
exports.watchResources = watchResources;

function getParentProcessResourceTypes(resourceTypes) {
  return resourceTypes.filter(resourceType => {
    return resourceType in ParentProcessResources;
  });
}
exports.getParentProcessResourceTypes = getParentProcessResourceTypes;

function getResourceTypesForTargetType(resourceTypes, targetType) {
  const resourceDictionnary = getResourceTypeDictionaryForTargetType(
    targetType
  );
  return resourceTypes.filter(resourceType => {
    return resourceType in resourceDictionnary;
  });
}
exports.getResourceTypesForTargetType = getResourceTypesForTargetType;

function hasResourceTypesForTargets(resourceTypes) {
  return resourceTypes.some(resourceType => {
    return resourceType in FrameTargetResources;
  });
}
exports.hasResourceTypesForTargets = hasResourceTypesForTargets;

/**
 * Stop watching for a list of resource types.
 *
 * @param Actor watcherOrTargetActor
 *        The related actor, already passed to watchResources.
 * @param Array<String> resourceTypes
 *        List of all type of resource to stop listening to.
 */
function unwatchResources(watcherOrTargetActor, resourceTypes) {
  for (const resourceType of resourceTypes) {
    // Pull all info about this resource type from `Resources` global object
    const { watchers } = getResourceTypeEntry(
      watcherOrTargetActor,
      resourceType
    );

    const watcher = watchers.get(watcherOrTargetActor);
    if (watcher) {
      watcher.destroy();
      watchers.delete(watcherOrTargetActor);
    }
  }
}
exports.unwatchResources = unwatchResources;

/**
 * Stop watching for all watched resources on a given actor.
 *
 * @param Actor watcherOrTargetActor
 *        The related actor, already passed to watchResources.
 */
function unwatchAllTargetResources(watcherOrTargetActor) {
  for (const { watchers } of Object.values(
    getResourceTypeDictionary(watcherOrTargetActor)
  )) {
    const watcher = watchers.get(watcherOrTargetActor);
    if (watcher) {
      watcher.destroy();
      watchers.delete(watcherOrTargetActor);
    }
  }
}
exports.unwatchAllTargetResources = unwatchAllTargetResources;

/**
 * If we are watching for the given resource type,
 * return the current ResourceWatcher instance used by this target actor
 * in order to observe this resource type.
 *
 * @param Actor watcherOrTargetActor
 *        Either a WatcherActor or a TargetActor which can be listening to a resource.
 *        WatcherActor will be used for resources listened from the parent process,
 *        and TargetActor will be used for resources listened from the content process.
 * @param String resourceType
 *        The resource type to query
 * @return ResourceWatcher
 *         The resource watcher instance, defined in devtools/server/actors/resources/
 */
function getResourceWatcher(watcherOrTargetActor, resourceType) {
  const { watchers } = getResourceTypeEntry(watcherOrTargetActor, resourceType);

  return watchers.get(watcherOrTargetActor);
}
exports.getResourceWatcher = getResourceWatcher;
