/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

<%namespace name="helpers" file="/helpers.mako.rs" />

<%
    from data import to_idl_name, SYSTEM_FONT_LONGHANDS, to_camel_case
    from itertools import groupby
%>

#[cfg(feature = "gecko")] use crate::gecko_bindings::structs::nsCSSPropertyID;
use crate::properties::{
    longhands::{
        self, visibility::computed_value::T as Visibility,
    },
    CSSWideKeyword, LonghandId, NonCustomPropertyIterator,
    PropertyDeclaration, PropertyDeclarationId,
};
#[cfg(feature = "gecko")] use crate::properties::{
    longhands::content_visibility::computed_value::T as ContentVisibility,
    NonCustomPropertyId,
};
use std::ptr;
use std::mem;
use fxhash::FxHashMap;
use super::ComputedValues;
use crate::properties::OwnedPropertyDeclarationId;
use crate::values::animated::{Animate, Procedure, ToAnimatedValue, ToAnimatedZero};
use crate::values::animated::effects::AnimatedFilter;
#[cfg(feature = "gecko")] use crate::values::computed::TransitionProperty;
use crate::values::computed::{ClipRect, Context};
use crate::values::computed::ToComputedValue;
use crate::values::distance::{ComputeSquaredDistance, SquaredDistance};
use crate::values::generics::effects::Filter;
use void::{self, Void};
use crate::properties_and_values::value::CustomAnimatedValue;

/// Convert nsCSSPropertyID to TransitionProperty
#[cfg(feature = "gecko")]
#[allow(non_upper_case_globals)]
impl From<nsCSSPropertyID> for TransitionProperty {
    fn from(property: nsCSSPropertyID) -> TransitionProperty {
        TransitionProperty::NonCustom(NonCustomPropertyId::from_nscsspropertyid(property).unwrap())
    }
}

/// A collection of AnimationValue that were composed on an element.
/// This HashMap stores the values that are the last AnimationValue to be
/// composed for each TransitionProperty.
pub type AnimationValueMap = FxHashMap<OwnedPropertyDeclarationId, AnimationValue>;

/// An enum to represent a single computed value belonging to an animated
/// property in order to be interpolated with another one. When interpolating,
/// both values need to belong to the same property.
#[derive(Debug, MallocSizeOf)]
#[repr(u16)]
pub enum AnimationValue {
    % for prop in data.longhands:
    /// `${prop.name}`
    % if prop.animatable and not prop.logical:
    ${prop.camel_case}(${prop.animated_type()}),
    % else:
    ${prop.camel_case}(Void),
    % endif
    % endfor
    /// A custom property.
    Custom(CustomAnimatedValue),
}

<%
    animated = []
    unanimated = []
    animated_with_logical = []
    for prop in data.longhands:
        if prop.animatable:
            animated_with_logical.append(prop)
        if prop.animatable and not prop.logical:
            animated.append(prop)
        else:
            unanimated.append(prop)
%>

#[repr(C)]
struct AnimationValueVariantRepr<T> {
    tag: u16,
    value: T
}

impl Clone for AnimationValue {
    #[inline]
    fn clone(&self) -> Self {
        use self::AnimationValue::*;

        <%
            [copy, others] = [list(g) for _, g in groupby(animated, key=lambda x: not x.specified_is_copy())]
        %>

        let self_tag = unsafe { *(self as *const _ as *const u16) };
        if self_tag <= LonghandId::${copy[-1].camel_case} as u16 {
            #[derive(Clone, Copy)]
            #[repr(u16)]
            enum CopyVariants {
                % for prop in copy:
                _${prop.camel_case}(${prop.animated_type()}),
                % endfor
            }

            unsafe {
                let mut out = mem::MaybeUninit::uninit();
                ptr::write(
                    out.as_mut_ptr() as *mut CopyVariants,
                    *(self as *const _ as *const CopyVariants),
                );
                return out.assume_init();
            }
        }

        match *self {
            % for ty, props in groupby(others, key=lambda x: x.animated_type()):
            <% props = list(props) %>
            ${" |\n".join("{}(ref value)".format(prop.camel_case) for prop in props)} => {
                % if len(props) == 1:
                ${props[0].camel_case}(value.clone())
                % else:
                unsafe {
                    let mut out = mem::MaybeUninit::uninit();
                    ptr::write(
                        out.as_mut_ptr() as *mut AnimationValueVariantRepr<${ty}>,
                        AnimationValueVariantRepr {
                            tag: *(self as *const _ as *const u16),
                            value: value.clone(),
                        },
                    );
                    out.assume_init()
                }
                % endif
            }
            % endfor
            Custom(ref animated_value) => Custom(animated_value.clone()),
            _ => unsafe { debug_unreachable!() }
        }
    }
}

impl PartialEq for AnimationValue {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        use self::AnimationValue::*;

        unsafe {
            let this_tag = *(self as *const _ as *const u16);
            let other_tag = *(other as *const _ as *const u16);
            if this_tag != other_tag {
                return false;
            }

            match *self {
                % for ty, props in groupby(animated, key=lambda x: x.animated_type()):
                ${" |\n".join("{}(ref this)".format(prop.camel_case) for prop in props)} => {
                    let other_repr =
                        &*(other as *const _ as *const AnimationValueVariantRepr<${ty}>);
                    *this == other_repr.value
                }
                % endfor
                ${" |\n".join("{}(void)".format(prop.camel_case) for prop in unanimated)} => {
                    void::unreachable(void)
                },
                AnimationValue::Custom(ref this) => {
                    let other_repr =
                        &*(other as *const _ as *const AnimationValueVariantRepr<CustomAnimatedValue>);
                    *this == other_repr.value
                },
            }
        }
    }
}

impl AnimationValue {
    /// Returns the longhand id this animated value corresponds to.
    #[inline]
    pub fn id(&self) -> PropertyDeclarationId {
        if let AnimationValue::Custom(animated_value) = self {
            return PropertyDeclarationId::Custom(&animated_value.name);
        }

        let id = unsafe { *(self as *const _ as *const LonghandId) };
        debug_assert_eq!(id, match *self {
            % for prop in data.longhands:
            % if prop.animatable and not prop.logical:
            AnimationValue::${prop.camel_case}(..) => LonghandId::${prop.camel_case},
            % else:
            AnimationValue::${prop.camel_case}(void) => void::unreachable(void),
            % endif
            % endfor
            AnimationValue::Custom(..) => unsafe { debug_unreachable!() },
        });
        PropertyDeclarationId::Longhand(id)
    }

    /// Returns whether this value is interpolable with another one.
    pub fn interpolable_with(&self, other: &Self) -> bool {
        self.animate(other, Procedure::Interpolate { progress: 0.5 }).is_ok()
    }

    /// "Uncompute" this animation value in order to be used inside the CSS
    /// cascade.
    pub fn uncompute(&self) -> PropertyDeclaration {
        use crate::properties::longhands;
        use self::AnimationValue::*;

        use super::PropertyDeclarationVariantRepr;

        match *self {
            <% keyfunc = lambda x: (x.base_type(), x.specified_type(), x.boxed, x.animation_type != "discrete") %>
            % for (ty, specified, boxed, to_animated), props in groupby(animated, key=keyfunc):
            <% props = list(props) %>
            ${" |\n".join("{}(ref value)".format(prop.camel_case) for prop in props)} => {
                % if to_animated:
                let value = ToAnimatedValue::from_animated_value(value.clone());
                % endif
                let value = ${ty}::from_computed_value(&value);
                % if boxed:
                let value = Box::new(value);
                % endif
                % if len(props) == 1:
                PropertyDeclaration::${props[0].camel_case}(value)
                % else:
                unsafe {
                    let mut out = mem::MaybeUninit::uninit();
                    ptr::write(
                        out.as_mut_ptr() as *mut PropertyDeclarationVariantRepr<${specified}>,
                        PropertyDeclarationVariantRepr {
                            tag: *(self as *const _ as *const u16),
                            value,
                        },
                    );
                    out.assume_init()
                }
                % endif
            }
            % endfor
            ${" |\n".join("{}(void)".format(prop.camel_case) for prop in unanimated)} => {
                void::unreachable(void)
            },
            Custom(ref animated_value) => animated_value.to_declaration(),
        }
    }

    /// Construct an AnimationValue from a property declaration.
    pub fn from_declaration(
        decl: &PropertyDeclaration,
        context: &mut Context,
        style: &ComputedValues,
        initial: &ComputedValues,
    ) -> Option<Self> {
        use super::PropertyDeclarationVariantRepr;

        <%
            keyfunc = lambda x: (
                x.specified_type(),
                x.animated_type(),
                x.boxed,
                x.animation_type not in ["discrete", "none"],
                x.style_struct.inherited,
                x.ident in SYSTEM_FONT_LONGHANDS and engine == "gecko",
            )
        %>

        let animatable = match *decl {
            % for (specified_ty, ty, boxed, to_animated, inherit, system), props in groupby(animated_with_logical, key=keyfunc):
            ${" |\n".join("PropertyDeclaration::{}(ref value)".format(prop.camel_case) for prop in props)} => {
                let decl_repr = unsafe {
                    &*(decl as *const _ as *const PropertyDeclarationVariantRepr<${specified_ty}>)
                };
                let longhand_id = unsafe {
                    *(&decl_repr.tag as *const u16 as *const LonghandId)
                };
                context.for_non_inherited_property = ${"false" if inherit else "true"};
                % if system:
                if let Some(sf) = value.get_system() {
                    longhands::system_font::resolve_system_font(sf, context)
                }
                % endif
                % if boxed:
                let value = (**value).to_computed_value(context);
                % else:
                let value = value.to_computed_value(context);
                % endif
                % if to_animated:
                let value = value.to_animated_value(&crate::values::animated::Context { style });
                % endif

                unsafe {
                    let mut out = mem::MaybeUninit::uninit();
                    ptr::write(
                        out.as_mut_ptr() as *mut AnimationValueVariantRepr<${ty}>,
                        AnimationValueVariantRepr {
                            tag: longhand_id.to_physical(context.builder.writing_mode) as u16,
                            value,
                        },
                    );
                    out.assume_init()
                }
            }
            % endfor
            PropertyDeclaration::CSSWideKeyword(ref declaration) => {
                match declaration.id.to_physical(context.builder.writing_mode) {
                    // We put all the animatable properties first in the hopes
                    // that it might increase match locality.
                    % for prop in data.longhands:
                    % if prop.animatable and not prop.logical:
                    LonghandId::${prop.camel_case} => {
                        // FIXME(emilio, bug 1533327): I think revert (and
                        // revert-layer) handling is not fine here, but what to
                        // do instead?
                        //
                        // Seems we'd need the computed value as if it was
                        // revert, somehow. Treating it as `unset` seems fine
                        // for now...
                        let style_struct = match declaration.keyword {
                            % if not prop.style_struct.inherited:
                            CSSWideKeyword::Revert |
                            CSSWideKeyword::RevertLayer |
                            CSSWideKeyword::Unset |
                            % endif
                            CSSWideKeyword::Initial => {
                                initial.get_${prop.style_struct.name_lower}()
                            },
                            % if prop.style_struct.inherited:
                            CSSWideKeyword::Revert |
                            CSSWideKeyword::RevertLayer |
                            CSSWideKeyword::Unset |
                            % endif
                            CSSWideKeyword::Inherit => {
                                context.builder
                                       .get_parent_${prop.style_struct.name_lower}()
                            },
                        };
                        let computed = style_struct
                        % if prop.logical:
                            .clone_${prop.ident}(context.builder.writing_mode);
                        % else:
                            .clone_${prop.ident}();
                        % endif

                        % if prop.animation_type != "discrete":
                        let computed = computed.to_animated_value(&crate::values::animated::Context {
                            style
                        });
                        % endif
                        AnimationValue::${prop.camel_case}(computed)
                    },
                    % endif
                    % endfor
                    % for prop in data.longhands:
                    % if not prop.animatable or prop.logical:
                    LonghandId::${prop.camel_case} => return None,
                    % endif
                    % endfor
                }
            },
            PropertyDeclaration::WithVariables(ref declaration) => {
                let mut cache = Default::default();
                let substituted = {
                    let custom_properties = &context.style().custom_properties();

                    debug_assert!(
                        context.builder.stylist.is_some(),
                        "Need a Stylist to substitute variables!"
                    );
                    declaration.value.substitute_variables(
                        declaration.id,
                        custom_properties,
                        context.builder.stylist.unwrap(),
                        context,
                        &mut cache,
                    )
                };
                return AnimationValue::from_declaration(
                    &substituted,
                    context,
                    style,
                    initial,
                )
            },
            PropertyDeclaration::Custom(ref declaration) => {
                AnimationValue::Custom(CustomAnimatedValue::from_declaration(
                    declaration,
                    context,
                    initial,
                )?)
            },
            _ => return None // non animatable properties will get included because of shorthands. ignore.
        };
        Some(animatable)
    }

    /// Get an AnimationValue for an declaration id from a given computed values.
    pub fn from_computed_values(
        property: PropertyDeclarationId,
        style: &ComputedValues,
    ) -> Option<Self> {
        let property = match property {
            PropertyDeclarationId::Longhand(id) => id,
            PropertyDeclarationId::Custom(ref name) => {
                // FIXME(bug 1869476): This should use a stylist to determine whether the name
                // corresponds to an inherited custom property and then choose the
                // inherited/non_inherited map accordingly.
                let p = &style.custom_properties();
                let value = p.inherited.get(*name).or_else(|| p.non_inherited.get(*name))?;
                return Some(AnimationValue::Custom(CustomAnimatedValue::from_computed(name, value)))
            }
        };

        Some(match property {
            % for prop in data.longhands:
            % if prop.animatable and not prop.logical:
            LonghandId::${prop.camel_case} => {
                let computed = style.clone_${prop.ident}();
                AnimationValue::${prop.camel_case}(
                % if prop.animation_type == "discrete":
                    computed
                % else:
                    computed.to_animated_value(&crate::values::animated::Context { style })
                % endif
                )
            }
            % endif
            % endfor
            _ => return None,
        })
    }

    /// Update `style` with the value of this `AnimationValue`.
    ///
    /// SERVO ONLY: This doesn't properly handle things like updating 'em' units
    /// when animated font-size.
    #[cfg(feature = "servo")]
    pub fn set_in_style_for_servo(&self, style: &mut ComputedValues) {
        match self {
            % for prop in data.longhands:
            % if prop.animatable and not prop.logical:
            AnimationValue::${prop.camel_case}(ref value) => {
                let value: longhands::${prop.ident}::computed_value::T =
                % if prop.animation_type != "discrete":
                    ToAnimatedValue::from_animated_value(value.clone());
                % else:
                    value.clone();
                % endif
                style.mutate_${prop.style_struct.name_lower}().set_${prop.ident}(value);
            }
            % else:
            AnimationValue::${prop.camel_case}(..) => unreachable!(),
            % endif
            % endfor
            AnimationValue::Custom(..) => unreachable!(),
        }
    }

    /// As above, but a stub for Gecko.
    #[cfg(feature = "gecko")]
    pub fn set_in_style_for_servo(&self, _: &mut ComputedValues) {
    }
}

fn animate_discrete<T: Clone>(this: &T, other: &T, procedure: Procedure) -> Result<T, ()> {
    if let Procedure::Interpolate { progress } = procedure {
        Ok(if progress < 0.5 { this.clone() } else { other.clone() })
    } else {
        Err(())
    }
}

impl Animate for AnimationValue {
    fn animate(&self, other: &Self, procedure: Procedure) -> Result<Self, ()> {
        Ok(unsafe {
            use self::AnimationValue::*;

            let this_tag = *(self as *const _ as *const u16);
            let other_tag = *(other as *const _ as *const u16);
            if this_tag != other_tag {
                panic!("Unexpected AnimationValue::animate call");
            }

            match *self {
                <% keyfunc = lambda x: (x.animated_type(), x.animation_type == "discrete") %>
                % for (ty, discrete), props in groupby(animated, key=keyfunc):
                ${" |\n".join("{}(ref this)".format(prop.camel_case) for prop in props)} => {
                    let other_repr =
                        &*(other as *const _ as *const AnimationValueVariantRepr<${ty}>);
                    % if discrete:
                    let value = animate_discrete(this, &other_repr.value, procedure)?;
                    % else:
                    let value = this.animate(&other_repr.value, procedure)?;
                    % endif

                    let mut out = mem::MaybeUninit::uninit();
                    ptr::write(
                        out.as_mut_ptr() as *mut AnimationValueVariantRepr<${ty}>,
                        AnimationValueVariantRepr {
                            tag: this_tag,
                            value,
                        },
                    );
                    out.assume_init()
                },
                % endfor
                ${" |\n".join("{}(void)".format(prop.camel_case) for prop in unanimated)} => {
                    void::unreachable(void)
                },
                Custom(ref self_value) => {
                    let Custom(ref other_value) = *other else { unreachable!() };
                    Custom(self_value.animate(other_value, procedure)?)
                },
            }
        })
    }
}

<%
    nondiscrete = []
    for prop in animated:
        if prop.animation_type != "discrete":
            nondiscrete.append(prop)
%>

impl ComputeSquaredDistance for AnimationValue {
    fn compute_squared_distance(&self, other: &Self) -> Result<SquaredDistance, ()> {
        unsafe {
            use self::AnimationValue::*;

            let this_tag = *(self as *const _ as *const u16);
            let other_tag = *(other as *const _ as *const u16);
            if this_tag != other_tag {
                panic!("Unexpected AnimationValue::compute_squared_distance call");
            }

            match *self {
                % for ty, props in groupby(nondiscrete, key=lambda x: x.animated_type()):
                ${" |\n".join("{}(ref this)".format(prop.camel_case) for prop in props)} => {
                    let other_repr =
                        &*(other as *const _ as *const AnimationValueVariantRepr<${ty}>);

                    this.compute_squared_distance(&other_repr.value)
                }
                % endfor
                _ => Err(()),
            }
        }
    }
}

impl ToAnimatedZero for AnimationValue {
    #[inline]
    fn to_animated_zero(&self) -> Result<Self, ()> {
        match *self {
            % for prop in data.longhands:
            % if prop.animatable and not prop.logical and prop.animation_type != "discrete":
            AnimationValue::${prop.camel_case}(ref base) => {
                Ok(AnimationValue::${prop.camel_case}(base.to_animated_zero()?))
            },
            % endif
            % endfor
            AnimationValue::Custom(..) => {
                // TODO(bug 1869185): For some non-universal registered custom properties, it may make sense to implement this.
                Err(())
            },
            _ => Err(()),
        }
    }
}

/// <https://drafts.csswg.org/web-animations-1/#animating-visibility>
impl Animate for Visibility {
    #[inline]
    fn animate(&self, other: &Self, procedure: Procedure) -> Result<Self, ()> {
        match procedure {
            Procedure::Interpolate { .. } => {
                let (this_weight, other_weight) = procedure.weights();
                match (*self, *other) {
                    (Visibility::Visible, _) => {
                        Ok(if this_weight > 0.0 { *self } else { *other })
                    },
                    (_, Visibility::Visible) => {
                        Ok(if other_weight > 0.0 { *other } else { *self })
                    },
                    _ => Err(()),
                }
            },
            _ => Err(()),
        }
    }
}

impl ComputeSquaredDistance for Visibility {
    #[inline]
    fn compute_squared_distance(&self, other: &Self) -> Result<SquaredDistance, ()> {
        Ok(SquaredDistance::from_sqrt(if *self == *other { 0. } else { 1. }))
    }
}

impl ToAnimatedZero for Visibility {
    #[inline]
    fn to_animated_zero(&self) -> Result<Self, ()> {
        Err(())
    }
}

/// <https://drafts.csswg.org/css-contain-3/#content-visibility-animation>
#[cfg(feature = "gecko")]
impl Animate for ContentVisibility {
    #[inline]
    fn animate(&self, other: &Self, procedure: Procedure) -> Result<Self, ()> {
        match procedure {
            Procedure::Interpolate { .. } => {
                let (this_weight, other_weight) = procedure.weights();
                match (*self, *other) {
                    (ContentVisibility::Hidden, _) => {
                        Ok(if other_weight > 0.0 { *other } else { *self })
                    },
                    (_, ContentVisibility::Hidden) => {
                        Ok(if this_weight > 0.0 { *self } else { *other })
                    },
                    _ => Err(()),
                }
            },
            _ => Err(()),
        }
    }
}

#[cfg(feature = "gecko")]
impl ComputeSquaredDistance for ContentVisibility {
    #[inline]
    fn compute_squared_distance(&self, other: &Self) -> Result<SquaredDistance, ()> {
        Ok(SquaredDistance::from_sqrt(if *self == *other { 0. } else { 1. }))
    }
}

#[cfg(feature = "gecko")]
impl ToAnimatedZero for ContentVisibility {
    #[inline]
    fn to_animated_zero(&self) -> Result<Self, ()> {
        Err(())
    }
}

/// <https://drafts.csswg.org/css-transitions/#animtype-rect>
impl Animate for ClipRect {
    #[inline]
    fn animate(&self, other: &Self, procedure: Procedure) -> Result<Self, ()> {
        use crate::values::computed::LengthOrAuto;
        let animate_component = |this: &LengthOrAuto, other: &LengthOrAuto| {
            let result = this.animate(other, procedure)?;
            if let Procedure::Interpolate { .. } = procedure {
                return Ok(result);
            }
            if result.is_auto() {
                // FIXME(emilio): Why? A couple SMIL tests fail without this,
                // but it seems extremely fishy.
                return Err(());
            }
            Ok(result)
        };

        Ok(ClipRect {
            top: animate_component(&self.top, &other.top)?,
            right: animate_component(&self.right, &other.right)?,
            bottom: animate_component(&self.bottom, &other.bottom)?,
            left: animate_component(&self.left, &other.left)?,
        })
    }
}

<%
    FILTER_FUNCTIONS = [ 'Blur', 'Brightness', 'Contrast', 'Grayscale',
                         'HueRotate', 'Invert', 'Opacity', 'Saturate',
                         'Sepia' ]
%>

/// <https://drafts.fxtf.org/filters/#animation-of-filters>
impl Animate for AnimatedFilter {
    fn animate(
        &self,
        other: &Self,
        procedure: Procedure,
    ) -> Result<Self, ()> {
        use crate::values::animated::animate_multiplicative_factor;
        match (self, other) {
            % for func in ['Blur', 'DropShadow', 'Grayscale', 'HueRotate', 'Invert', 'Sepia']:
            (&Filter::${func}(ref this), &Filter::${func}(ref other)) => {
                Ok(Filter::${func}(this.animate(other, procedure)?))
            },
            % endfor
            % for func in ['Brightness', 'Contrast', 'Opacity', 'Saturate']:
            (&Filter::${func}(this), &Filter::${func}(other)) => {
                Ok(Filter::${func}(animate_multiplicative_factor(this, other, procedure)?))
            },
            % endfor
            _ => Err(()),
        }
    }
}

/// <http://dev.w3.org/csswg/css-transforms/#none-transform-animation>
impl ToAnimatedZero for AnimatedFilter {
    fn to_animated_zero(&self) -> Result<Self, ()> {
        match *self {
            % for func in ['Blur', 'DropShadow', 'Grayscale', 'HueRotate', 'Invert', 'Sepia']:
            Filter::${func}(ref this) => Ok(Filter::${func}(this.to_animated_zero()?)),
            % endfor
            % for func in ['Brightness', 'Contrast', 'Opacity', 'Saturate']:
            Filter::${func}(_) => Ok(Filter::${func}(1.)),
            % endfor
            _ => Err(()),
        }
    }
}

/// An iterator over all the properties that transition on a given style.
pub struct TransitionPropertyIterator<'a> {
    style: &'a ComputedValues,
    index_range: core::ops::Range<usize>,
    longhand_iterator: Option<NonCustomPropertyIterator<LonghandId>>,
}

impl<'a> TransitionPropertyIterator<'a> {
    /// Create a `TransitionPropertyIterator` for the given style.
    pub fn from_style(style: &'a ComputedValues) -> Self {
        Self {
            style,
            index_range: 0..style.get_ui().transition_property_count(),
            longhand_iterator: None,
        }
    }
}

/// A single iteration of the TransitionPropertyIterator.
pub struct TransitionPropertyIteration {
    /// The id of the longhand for this property.
    pub property: OwnedPropertyDeclarationId,

    /// The index of this property in the list of transition properties for this
    /// iterator's style.
    pub index: usize,
}

impl<'a> Iterator for TransitionPropertyIterator<'a> {
    type Item = TransitionPropertyIteration;

    fn next(&mut self) -> Option<Self::Item> {
        use crate::values::computed::TransitionProperty;
        loop {
            if let Some(ref mut longhand_iterator) = self.longhand_iterator {
                if let Some(longhand_id) = longhand_iterator.next() {
                    return Some(TransitionPropertyIteration {
                        property: OwnedPropertyDeclarationId::Longhand(longhand_id),
                        index: self.index_range.start - 1,
                    });
                }
                self.longhand_iterator = None;
            }

            let index = self.index_range.next()?;
            match self.style.get_ui().transition_property_at(index) {
                TransitionProperty::NonCustom(id) => {
                    match id.longhand_or_shorthand() {
                        Ok(longhand_id) => {
                            return Some(TransitionPropertyIteration {
                                property: OwnedPropertyDeclarationId::Longhand(longhand_id),
                                index,
                            });
                        },
                        Err(shorthand_id) => {
                            // In the other cases, we set up our state so that we are ready to
                            // compute the next value of the iterator and then loop (equivalent
                            // to calling self.next()).
                            self.longhand_iterator = Some(shorthand_id.longhands());
                        },
                    }
                }
                TransitionProperty::Custom(name) => {
                    return Some(TransitionPropertyIteration {
                        property: OwnedPropertyDeclarationId::Custom(name),
                        index,
                    })
                },
                TransitionProperty::Unsupported(..) => {},
            }
        }
    }
}
